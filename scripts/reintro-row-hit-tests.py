"""Reintroduction check for the five row/line hit-test fixes.

Puts each fixed defect back one at a time and asserts that at least one test
fails. A defect that can be restored with every test still green is a defect
the suite does not actually pin -- which is exactly what "N tests pass" hides.

These five apps were found by a tree-wide audit for one shape of mistake: the
*top* of a scrollable row area gets spelled out once per pointer path plus once
in the renderer, while the *bottom* is spelled only once, in the renderer's
clip. The clip therefore stops above the status bar and the hit tests do not,
so a click on the status bar selects a row that was never drawn there.

    sysmonitor    right-clicking the status bar opened the Kill menu over an
                  arbitrary process
    procexplorer  the same, in the same words
    credmanager   clicking the "60 entries" caption decrypted and displayed
                  the first credential -- a negative offset cast to `usize`
                  saturates to 0 in Rust, it does not wrap
    musicplayer   the renderer snapped the list to whole rows while the hit
                  test worked at the continuous offset, so any trackpad scroll
                  put a different track under the pointer than was painted
                  there
    hexeditor     clicking the status bar jumped the cursor to the end of the
                  file, and clicking the data inspector -- painted *over* the
                  dump's right-hand side -- moved the cursor in the file behind
                  the panel
    settings      the sidebar's category list had no live fault, but its top
                  edge was written out longhand four times and the fourth copy
                  was in the test, which recomputed the constant and probed row
                  *centres* -- so the renderer and the hit test could drift
                  three pixels apart with the suite still green

The fix in every case is the same collapse: a `rows_top()`, a `rows_height()`
and a `row_at()` that every pointer path *and* the renderer's clip go through,
so the region drawn is the region clicked. What this script pins is that the
collapse cannot be quietly undone.

Run from the lane-c worktree root:
    python scripts/reintro-row-hit-tests.py

Set `REINTRO_TARGET_DIR` to reuse a warm build directory.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

SYSMONITOR = Path("apps/sysmonitor/src/main.rs")
PROCEXPLORER = Path("apps/procexplorer/src/main.rs")
CREDMANAGER = Path("apps/credmanager/src/main.rs")
MUSICPLAYER = Path("apps/musicplayer/src/main.rs")
HEXEDITOR = Path("apps/hexeditor/src/main.rs")
SETTINGS = Path("apps/settings/src/main.rs")

# (crate, path, name, old, new) -- `old` must occur exactly once in `path`.
DEFECTS = [
    # ------------------------------------------------------------ sysmonitor
    (
        "sysmonitor",
        SYSMONITOR,
        "the process list is hit-tested under its own status bar",
        "        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {",
        "        if !offset.is_finite() || offset < 0.0 {",
    ),
    # ---------------------------------------------------------- procexplorer
    (
        "procexplorer",
        PROCEXPLORER,
        "the process list is hit-tested under its own status bar",
        "        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {",
        "        if !offset.is_finite() || offset < 0.0 {",
    ),
    (
        "procexplorer",
        PROCEXPLORER,
        "the process rows answer a click made on another tab",
        "        if self.active_tab != Tab::Processes {\n            return None;\n        }\n",
        "",
    ),
    # ----------------------------------------------------------- credmanager
    (
        "credmanager",
        CREDMANAGER,
        'clicking the "N entries" caption opens the first credential',
        "        let offset = my - Self::rows_top();\n"
        "        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {\n"
        "            return None;\n"
        "        }\n"
        "        let from_top = offset + self.list_scroll;\n"
        "        if !from_top.is_finite() || from_top < 0.0 {\n"
        "            return None;\n"
        "        }\n",
        "        let offset = my - Self::rows_top();\n"
        "        let from_top = offset + self.list_scroll;\n",
    ),
    (
        "credmanager",
        CREDMANAGER,
        "the entry list is hit-tested below its own bottom edge",
        "        if !offset.is_finite() || offset < 0.0 || offset >= self.rows_height() {",
        "        if !offset.is_finite() || offset < 0.0 {",
    ),
    # ----------------------------------------------------------- musicplayer
    (
        "musicplayer",
        MUSICPLAYER,
        "the library is drawn snapped to whole rows and hit-tested continuously",
        "    let origin_y = rows_y - state.scroll_offset;\n"
        "\n"
        "    tree.clip(0.0, rows_y, state.width, rows_h);\n"
        "\n"
        "    for track_idx in first..filtered.len() {",
        "    #[allow(clippy::cast_precision_loss)]\n"
        "    let origin_y = rows_y - (first as f32) * TRACK_ROW_HEIGHT;\n"
        "\n"
        "    tree.clip(0.0, rows_y, state.width, rows_h);\n"
        "\n"
        "    for track_idx in first..filtered.len() {",
    ),
    (
        "musicplayer",
        MUSICPLAYER,
        "the playlist is drawn snapped to whole rows and hit-tested continuously",
        "    let origin_y = rows_y - state.scroll_offset;\n"
        "\n"
        "    tree.clip(0.0, rows_y, state.width, rows_h);\n"
        "\n"
        "    for track_idx in first..state.playlist.len() {",
        "    #[allow(clippy::cast_precision_loss)]\n"
        "    let origin_y = rows_y - (first as f32) * TRACK_ROW_HEIGHT;\n"
        "\n"
        "    tree.clip(0.0, rows_y, state.width, rows_h);\n"
        "\n"
        "    for track_idx in first..state.playlist.len() {",
    ),
    (
        "musicplayer",
        MUSICPLAYER,
        "the selected track index has no upper bound",
        "        let idx = (from_top / TRACK_ROW_HEIGHT) as usize;\n"
        "        if idx < self.row_count() {\n"
        "            Some(idx)\n"
        "        } else {\n"
        "            None\n"
        "        }\n",
        "        let idx = (from_top / TRACK_ROW_HEIGHT) as usize;\n        Some(idx)\n",
    ),
    (
        "musicplayer",
        MUSICPLAYER,
        "the scroll offset is bounded only by the wheel handler",
        "    if state.scroll_offset > 0.0 || !state.scroll_offset.is_finite() {\n"
        "        state.scroll_offset = state.scroll_offset.clamp(0.0, state.max_scroll());\n"
        "    }\n",
        "",
    ),
    # ------------------------------------------------------------- hexeditor
    (
        "hexeditor",
        HEXEDITOR,
        "clicking the status bar jumps the cursor to the end of the file",
        "        let from_top = y - Self::content_top();\n"
        "        if !from_top.is_finite() || from_top < 0.0 || from_top >= self.content_height() {\n"
        "            return None;\n"
        "        }\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let slot = (from_top / LINE_HEIGHT) as usize;\n"
        "        if slot >= self.visible_lines() {\n"
        "            return None;\n"
        "        }\n",
        "        let from_top = y - Self::content_top();\n"
        "        if !from_top.is_finite() || from_top < 0.0 {\n"
        "            return None;\n"
        "        }\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let slot = (from_top / LINE_HEIGHT) as usize;\n",
    ),
    (
        "hexeditor",
        HEXEDITOR,
        "clicking the data inspector moves the cursor in the file behind it",
        "        if !x.is_finite() || x < 0.0 || x >= self.content_width() {",
        "        if !x.is_finite() || x < 0.0 {",
    ),
    (
        "hexeditor",
        HEXEDITOR,
        "the wheel reads only the sign of dy, and reads it backwards",
        "        let delta = doc.view.wheel.rows(dy);\n"
        "        doc.view.scroll_offset =\n"
        "            guitk::scroll_window::shift(doc.view.scroll_offset, delta).min(max_scroll);\n",
        "        let _unused = &doc.view.wheel;\n"
        "        let up = if dy < 0.0 { 3usize } else { 0usize };\n"
        "        let down = if dy > 0.0 { 3usize } else { 0usize };\n"
        "        if up > 0 {\n"
        "            doc.view.scroll_offset = doc.view.scroll_offset.saturating_sub(up);\n"
        "        }\n"
        "        if down > 0 {\n"
        "            doc.view.scroll_offset =\n"
        "                doc.view.scroll_offset.saturating_add(down).min(max_scroll);\n"
        "        }\n",
    ),
    # -------------------------------------------------------------- settings
    (
        "settings",
        SETTINGS,
        "the sidebar rows are drawn three pixels below where they answer",
        "            let item_y = Self::category_row_top(idx);",
        "            let item_y = Self::category_row_top(idx) + 3.0;",
    ),
    (
        "settings",
        SETTINGS,
        "the gap between two sidebar rows answers for the row above it",
        "        if my >= Self::category_row_top(idx) + Self::CATEGORY_ROW_PAINTED_HEIGHT {\n"
        "            return None;\n"
        "        }\n",
        "",
    ),
    (
        "settings",
        SETTINGS,
        "the hover highlight keeps its own copy of the row arithmetic",
        "            self.sidebar_hovered = Self::category_at(mx, my);\n",
        "            let list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0;\n"
        "            if my >= list_y {\n"
        "                let idx = ((my - list_y) / CATEGORY_ITEM_HEIGHT) as usize;\n"
        "                if idx < SettingsCategory::ALL.len() {\n"
        "                    self.sidebar_hovered = Some(idx);\n"
        "                } else {\n"
        "                    self.sidebar_hovered = None;\n"
        "                }\n"
        "            } else {\n"
        "                self.sidebar_hovered = None;\n"
        "            }\n",
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
        # Its own build directory by default: two cargo runs sharing one
        # `CARGO_TARGET_DIR` block on the same lock, so a reintroduction sweep
        # started next to a workspace test would simply hang. Override with
        # `REINTRO_TARGET_DIR` to reuse a warm one.
        env={
            **os.environ,
            "CARGO_TARGET_DIR": os.environ.get("REINTRO_TARGET_DIR", "target-reintro-c"),
        },
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
                print(f"SETUP ERROR: {crate}: {name!r} anchor occurs {count} times")
                bad.append(f"{crate}: {name}")
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
                print(f"NOT PINNED: {crate}: {name}")
                bad.append(f"{crate}: {name}")
            else:
                print(f"pinned by: {failures or '(build/other failure)'}  <- {crate}: {name}")
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

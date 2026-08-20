"""Reintroduction check for the row/line hit-test fixes.

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

The toolkit's two menu implementations are the same family in a harder form.
Their rows are *not* all one height -- a separator is 9 px where an item is 28
-- so there is no closed form to get right, only a walk, and each file carried
four of them: one summing the heights for the popup's total, one placing the
rows on screen, one subtracting them back off to answer a click, and one
adding them up again to decide where a submenu hangs. Four walks of one list
is four chances for three of them to be right. Both now read a single
`guitk::row_strip::RowStrip`, and the entries below pin each of the three
walks that used to be able to drift away from the renderer:

    guitk/menu     the context menu, used by every right-click in the OS
    guitk/menubar  the dropdowns under File/Edit/View in every windowed app

`guitk/tree` is the plain form of the same thing and had a live fault of its
own: `handle_click` and `handle_context_menu` each carried

    ((y + self.scroll_offset) / self.config.row_height) as usize

with no lower bound, so a click *above* the tree selected its first node --
the credmanager fault verbatim, in the toolkit, where every tree widget in the
OS inherits it. Its rows are all one height, so it uses a plain `row_at` and
not a `RowStrip`: for a uniform list `top + i * H` and `(y - top) / H` are
visibly each other's inverse, and the module doc of `row_strip` says as much.

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
MENU = Path("gui/toolkit/src/menu.rs")
MENUBAR = Path("gui/toolkit/src/menubar.rs")
TREE = Path("gui/toolkit/src/tree.rs")

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
    # ------------------------------------------------------------ guitk/menu
    (
        "guitk",
        MENU,
        "the context menu draws its rows three pixels below where they answer",
        "            let Some(current_y) = strip.top(i) else {",
        "            let Some(current_y) = strip.top(i).map(|t| t + 3.0) else {",
    ),
    (
        "guitk",
        MENU,
        "index_at_y keeps its own walk and sizes a separator as an item",
        "        let idx = self.strip().index_at(py)?;\n",
        "        let mut current_y = self.y + VERTICAL_PADDING;\n"
        "        let mut idx = self.items.len();\n"
        "        for (i, item) in self.items.iter().enumerate() {\n"
        "            let h = match item {\n"
        "                MenuItem::Separator => ITEM_HEIGHT,\n"
        "                _ => ITEM_HEIGHT,\n"
        "            };\n"
        "            if py >= current_y && py < current_y + h {\n"
        "                idx = i;\n"
        "                break;\n"
        "            }\n"
        "            current_y += h;\n"
        "        }\n"
        "        if idx >= self.items.len() {\n"
        "            return None;\n"
        "        }\n",
    ),
    (
        "guitk",
        MENU,
        "y_offset_for_index walks again and forgets separators take space",
        "        let strip = self.strip();\n"
        "        strip.top(target).unwrap_or_else(|| strip.bottom()) - self.y\n",
        "        let mut offset = VERTICAL_PADDING;\n"
        "        for (i, item) in self.items.iter().enumerate() {\n"
        "            if i == target {\n"
        "                return offset;\n"
        "            }\n"
        "            offset += match item {\n"
        "                MenuItem::Separator => 0.0,\n"
        "                _ => ITEM_HEIGHT,\n"
        "            };\n"
        "        }\n"
        "        offset\n",
    ),
    # --------------------------------------------------------- guitk/menubar
    (
        "guitk",
        MENUBAR,
        "the dropdown draws its rows three pixels below where they answer",
        "        let Some(cur_y) = strip.top(i) else {",
        "        let Some(cur_y) = strip.top(i).map(|t| t + 3.0) else {",
    ),
    (
        "guitk",
        MENUBAR,
        "item_index_at_y keeps its own walk and sizes a separator as an item",
        "    let idx = entry_strip(entries, 0.0).index_at(rel_y)?;\n",
        "    let mut cur = 0.0_f32;\n"
        "    let mut idx = entries.len();\n"
        "    for (i, entry) in entries.iter().enumerate() {\n"
        "        let h = match entry {\n"
        "            MenuBarEntry::Separator => ITEM_HEIGHT,\n"
        "            _ => ITEM_HEIGHT,\n"
        "        };\n"
        "        if rel_y >= cur && rel_y < cur + h {\n"
        "            idx = i;\n"
        "            break;\n"
        "        }\n"
        "        cur += h;\n"
        "    }\n"
        "    if idx >= entries.len() {\n"
        "        return None;\n"
        "    }\n",
    ),
    (
        "guitk",
        MENUBAR,
        "the dropdown's y_offset_for_index walks again and forgets separators",
        "    let strip = entry_strip(entries, 0.0);\n"
        "    strip.top(target).unwrap_or_else(|| strip.bottom())\n",
        "    let mut offset = 0.0_f32;\n"
        "    for (i, entry) in entries.iter().enumerate() {\n"
        "        if i == target {\n"
        "            return offset;\n"
        "        }\n"
        "        offset += match entry {\n"
        "            MenuBarEntry::Separator => 0.0,\n"
        "            _ => ITEM_HEIGHT,\n"
        "        };\n"
        "    }\n"
        "    offset\n",
    ),
    # ------------------------------------------------------------ guitk/tree
    (
        "guitk",
        TREE,
        "the tree names row 0 for a click above it, a NaN, or a zero row height",
        "        let row_h = self.config.row_height;\n"
        "        if !row_h.is_finite() || row_h <= 0.0 {\n"
        "            return None;\n"
        "        }\n"
        "        let content_y = y + self.scroll_offset;\n"
        "        if !content_y.is_finite() || content_y < 0.0 {\n"
        "            return None;\n"
        "        }\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let idx = (content_y / row_h) as usize;\n",
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let idx = ((y + self.scroll_offset) / self.config.row_height) as usize;\n",
    ),
    (
        "guitk",
        TREE,
        "the tree draws its rows three pixels below where they answer",
        "            let row_y = y + (idx as f32 * row_h) - self.scroll_offset;",
        "            let row_y = y + (idx as f32 * row_h) - self.scroll_offset + 3.0;",
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


def crates_are_green() -> tuple[bool, str]:
    """Do the crates under test pass *before* anything is reintroduced?

    A red baseline makes every result meaningless: the script decides a defect
    is pinned by observing that the suite goes from green to red, so a suite
    that was already red "pins" everything, including defects nothing tests.

    This is not hypothetical. A run made while a half-finished edit sat in the
    working tree reported two guitk defects as `pinned by:
    error[E0599]: no method named ...` -- a compile failure from an unrelated
    file, credited as evidence. It exited 0. Only the fallback that prints the
    compiler's own words made it visible at all, and that is a thing a reader
    has to notice rather than a thing the script refuses to do.
    """
    for crate in sorted({crate for crate, _, _, _, _ in DEFECTS}):
        passed, failures = run_tests(crate)
        if not passed:
            return False, f"{crate}: {failures or '(build/other failure)'}"
    return True, ""


def main() -> int:
    baseline_ok, why = crates_are_green()
    if not baseline_ok:
        print(f"BASELINE NOT GREEN -- {why}")
        print(
            "\nRefusing to run: every 'pinned by' below would be evidence of\n"
            "nothing, because the suite is red before any defect goes back in.\n"
            "Fix the tree first."
        )
        return 2

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

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

`partmanager` and the desktop's notification pane are the multi-consumer form:
one rectangle described from memory by three or four separate callers.

    partmanager   the partition list's renderer, click and wheel each spelled
                  out its top and its height, and the clip already ran 22 px
                  past the bottom a click accepted -- invisible only because
                  the queue panel is painted afterwards and covered the
                  overdraw, which is paint order doing a hit test's job. Its
                  operation queue panel below is the same again with four
                  consumers, and `28.0` -- the header height that
                  `QUEUE_HEADER_HEIGHT` already names -- written out five
                  times. There the wheel accepted the header while the hover
                  did not, so a notch over a *collapsed* panel (which is
                  nothing but its header) scrolled it against a zero-tall
                  viewport, to anywhere at all. Its disk sidebar is the third
                  and quietest instance: three spellings of `top + 28.0` and
                  two of the divide-and-cast, none of which had drifted yet.
                  A `DiskSidebar` now holds them together and, in doing so,
                  writes down two things the loose copies only implied -- that
                  the 4 px inset and the 2 px gap around a painted row are
                  decoration and not a boundary, so a row owns its full pitch
                  and the column's full width; and that the "Disks" caption
                  consumes the pointer even though it names no disk, so a
                  click there cannot reach the disk map behind it
    notif_pane    the per-app settings list's click divided by the card pitch,
                  handing each inter-card gutter to the card above it, and had
                  no bottom bound -- so a click below the pane, aimed at
                  another window, toggled an app that was never drawn there

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
PARTMANAGER = Path("apps/partmanager/src/main.rs")
REMOTEDESKTOP = Path("apps/remotedesktop/src/main.rs")
NOTIF_PANE = Path("gui/desktop/src/notif_pane.rs")

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
    # ----------------------------------------------------------- partmanager
    (
        "partmanager",
        PARTMANAGER,
        "the partition list is clipped 22 px below where a click is accepted",
        "        width: list_width,\n        height: geom.viewport_height(),",
        "        width: list_width,\n"
        "        height: (geom.bottom - geom.panel_top - 20.0).max(0.0),",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the click accepts a padding-wide strip right of the painted list",
        "    let list = PartitionList::of(app);\n"
        "    if list.contains(x, y) {\n"
        "        return handle_partition_list_click(app, y, list);",
        "    let list = PartitionList::of(app);\n"
        "    if y >= list.data_top\n"
        "        && y < list.bottom\n"
        "        && x >= list.left\n"
        "        && x < app.width - DETAIL_PANEL_WIDTH\n"
        "    {\n"
        "        return handle_partition_list_click(app, y, list);",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the wheel believes the viewport two pixels taller than it is",
        "        let max_scroll = list.max_scroll(region_count);",
        "        let max_scroll = ((region_count as f32) * PARTITION_ROW_HEIGHT\n"
        "            - (list.bottom - list.panel_top - 40.0))\n"
        "            .max(0.0);",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the wheel scrolls the gutter left of the list",
        "    let list = PartitionList::of(app);\n"
        "    if list.contains(x, y) {\n"
        "        let region_count",
        "    let list = PartitionList::of(app);\n"
        "    if y >= list.data_top\n"
        "        && y < list.bottom\n"
        "        && x >= SIDEBAR_WIDTH\n"
        "        && x < app.width - DETAIL_PANEL_WIDTH\n"
        "    {\n"
        "        let region_count",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the partition list's row_at casts an unchecked f32 straight to usize",
        "        if !(y >= self.data_top && y < self.bottom) {\n"
        "            return None;\n"
        "        }\n"
        "        let offset = y - self.data_top + scroll;\n"
        "        if !offset.is_finite() || offset < 0.0 {\n"
        "            return None;\n"
        "        }\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let index = (offset / PARTITION_ROW_HEIGHT) as usize;\n",
        "        let offset = y - self.data_top + scroll;\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let index = (offset / PARTITION_ROW_HEIGHT) as usize;\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the partition list draws its rows three pixels below where they answer",
        "        let ry = geom.row_y(i, app.partition_scroll);",
        "        let ry = geom.row_y(i, app.partition_scroll) + 3.0;",
    ),
    # The operation queue panel below the partition list is the same shape
    # again, with four consumers rather than three -- renderer, header click,
    # row hover and wheel -- and `28.0`, the header's height, written out five
    # times even though `QUEUE_HEADER_HEIGHT` already named it.
    (
        "partmanager",
        PARTMANAGER,
        "the queue's row_at casts an unchecked f32 straight to usize",
        "        if !(y >= self.data_top && y < self.bottom) {\n"
        "            return None;\n"
        "        }\n"
        "        let offset = y - self.data_top + scroll;\n"
        "        if !offset.is_finite() || offset < 0.0 {\n"
        "            return None;\n"
        "        }\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let index = (offset / QUEUE_ROW_HEIGHT) as usize;\n",
        "        let offset = y - self.data_top + scroll;\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let index = (offset / QUEUE_ROW_HEIGHT) as usize;\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the queue panel is clipped a row past where the pointer is accepted",
        "        width: panel_width,\n        height: geom.viewport_height(),",
        "        width: panel_width,\n"
        "        height: geom.viewport_height() + QUEUE_ROW_HEIGHT,",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the queue panel draws its rows three pixels below where they answer",
        "        let ry = geom.row_y(i, app.queue_scroll);",
        "        let ry = geom.row_y(i, app.queue_scroll) + 3.0;",
    ),
    (
        # The header and the collapsed panel are the same defect: a shut panel
        # *is* its header, so a wheel that accepts the header scrolls a panel
        # whose viewport is zero pixels tall -- and therefore to anywhere.
        "partmanager",
        PARTMANAGER,
        "the wheel accepts the queue's header, and so a collapsed panel too",
        "    let queue = QueueList::of(app);\n"
        "    if queue.contains(x, y) {\n"
        "        let max_scroll = QueueList::max_scroll(app.operation_queue.len());\n",
        "    let queue = QueueList::of(app);\n"
        "    if y >= queue.panel_top && y < queue.bottom && x >= SIDEBAR_WIDTH {\n"
        "        let max_scroll = (app.operation_queue.len() as f32 * QUEUE_ROW_HEIGHT\n"
        "            - queue.viewport_height())\n"
        "        .max(0.0);\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the queue's scroll stop leaves the header out of the viewport",
        "        ((count as f32) * QUEUE_ROW_HEIGHT"
        " - (QUEUE_PANEL_HEIGHT - QUEUE_HEADER_HEIGHT)).max(0.0)",
        "        ((count as f32) * QUEUE_ROW_HEIGHT - QUEUE_PANEL_HEIGHT).max(0.0)",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the queue's header click has no right-hand bound",
        "    if QueueList::of(app).contains_header(x, y) {",
        "    let q = QueueList::of(app);\n"
        "    if y >= q.panel_top && y < q.data_top && x >= SIDEBAR_WIDTH {",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the queue's hover has no right-hand bound and divides without a guard",
        "    let queue = QueueList::of(app);\n"
        "    if queue.contains(x, y) {\n"
        "        app.hovered_queue_row = "
        "queue.row_at(y, app.queue_scroll, app.operation_queue.len());\n",
        "    let queue = QueueList::of(app);\n"
        "    if y >= queue.data_top && y < queue.bottom && x >= SIDEBAR_WIDTH {\n"
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let row = ((y - queue.data_top + app.queue_scroll) / QUEUE_ROW_HEIGHT) as usize;\n"
        "        if row < app.operation_queue.len() {\n"
        "            app.hovered_queue_row = Some(row);\n"
        "        }\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "undo shortens the queue without pulling the panel back onto its rows",
        "        self.clamp_queue_scroll();\n        op\n",
        "        op\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "clearing the queue leaves the panel scrolled past an empty list",
        '        self.status_message = String::from("Operation queue cleared");\n'
        "        self.clamp_queue_scroll();\n",
        '        self.status_message = String::from("Operation queue cleared");\n',
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar's row walk answers above its own first row",
        "        if !(y >= self.data_top && y < self.bottom) {\n"
        "            return None;\n"
        "        }\n"
        "        let offset = y - self.data_top;\n"
        "        if !offset.is_finite() || offset < 0.0 {\n"
        "            return None;\n"
        "        }\n",
        "        if y >= self.bottom {\n"
        "            return None;\n"
        "        }\n"
        "        let offset = y - self.data_top;\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar draws its disks three pixels below where they answer",
        "            self.row_y(index),\n",
        "            self.row_y(index) + 3.0,\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar paces its rows by the height it paints, not by their pitch",
        "        let (rx, ry, rw, rh) = geom.row_paint_rect(i);\n",
        "        let (rx, _, rw, rh) = geom.row_paint_rect(i);\n"
        "        let ry = geom.data_top + (i as f32) * rh;\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar is clipped a row short of where the pointer is accepted",
        "        x: geom.left,\n"
        "        y: geom.data_top,\n"
        "        width: geom.width,\n"
        "        height: geom.viewport_height(),\n",
        "        x: geom.left,\n"
        "        y: geom.data_top,\n"
        "        width: geom.width,\n"
        "        height: geom.viewport_height() - SIDEBAR_DISK_ROW_HEIGHT,\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar click has no right-hand bound",
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if sidebar.contains_column(x, y) {\n"
        "        if let Some(row) = sidebar.row_at(y, app.disks.len()) {\n",
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if y >= sidebar.panel_top && y < sidebar.bottom {\n"
        "        if let Some(row) = sidebar.row_at(y, app.disks.len()) {\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar hover has no right-hand bound",
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if sidebar.contains_column(x, y) {\n"
        "        app.hovered_sidebar_disk = sidebar.row_at(y, app.disks.len());\n",
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if y >= sidebar.panel_top && y < sidebar.bottom {\n"
        "        app.hovered_sidebar_disk = sidebar.row_at(y, app.disks.len());\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        'a click on the "Disks" caption falls through to the disk map behind it',
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if sidebar.contains_column(x, y) {\n"
        "        if let Some(row) = sidebar.row_at(y, app.disks.len()) {\n",
        "    let sidebar = DiskSidebar::of(app);\n"
        "    if sidebar.contains(x, y) {\n"
        "        if let Some(row) = sidebar.row_at(y, app.disks.len()) {\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the blank gap under a sidebar row is treated as a boundary, not decoration",
        "        let index = (offset / SIDEBAR_DISK_ROW_HEIGHT) as usize;\n",
        "        let index = (offset / SIDEBAR_DISK_ROW_HEIGHT) as usize;\n"
        "        if offset % SIDEBAR_DISK_ROW_HEIGHT\n"
        "            >= SIDEBAR_DISK_ROW_HEIGHT - Self::ROW_GAP_Y\n"
        "        {\n"
        "            return None;\n"
        "        }\n",
    ),
    (
        "partmanager",
        PARTMANAGER,
        "the sidebar's side inset is treated as a boundary, not decoration",
        "    fn contains_column(&self, x: f32, y: f32) -> bool {\n"
        "        x >= self.left && x < self.right() && y >= self.panel_top && y < self.bottom\n",
        "    fn contains_column(&self, x: f32, y: f32) -> bool {\n"
        "        x >= self.left + Self::ROW_INSET_X\n"
        "            && x < self.right() - Self::ROW_INSET_X\n"
        "            && y >= self.panel_top\n"
        "            && y < self.bottom\n",
    ),
    # -------------------------------------------------------------- settings
    (
        "settings",
        SETTINGS,
        "the dropdown lets a NaN through every one of its bounds tests",
        "        if !mx.is_finite() || !my.is_finite() {\n"
        "            return None;\n"
        "        }\n"
        "        if mx < self.x",
        "        if mx < self.x",
    ),
    (
        "settings",
        SETTINGS,
        "the dropdown renderer drifts three pixels from its own hit test",
        "            let iy = layout.row_top(row);",
        "            let iy = layout.row_top(row) + 3.0;",
    ),
    (
        "settings",
        SETTINGS,
        "the dropdown test recovers row tops with a stale baseline offset",
        "    const DRAWN_ITEM_TEXT_BASELINE: f32 = 10.0;",
        "    const DRAWN_ITEM_TEXT_BASELINE: f32 = 13.0;",
    ),
    # --------------------------------------------------------- remotedesktop
    (
        "remotedesktop",
        REMOTEDESKTOP,
        "the sidebar guard is `y < 0.0`, which a NaN passes by failing it",
        "        if !y.is_finite() || y < 0.0 {",
        "        if y < 0.0 {",
    ),
    # ------------------------------------------- desktop / notification pane
    (
        "desktop",
        NOTIF_PANE,
        "the per-app click accepts the pitch, giving each gutter to the card above",
        "            local_y >= top && local_y < top + APP_CARD_HEIGHT",
        "            local_y >= top && local_y < top + APP_CARD_PITCH",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the enabled pill's hit test has no right edge",
        "        if rx < pill_x || rx >= pill_x + pill_w || local_y < pill_y"
        " || local_y >= pill_y + pill_h {",
        "        if rx < pill_x || local_y < pill_y || local_y >= pill_y + pill_h {",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the pill's y band starts at the card's top and runs 35 px",
        "        if rx < pill_x || rx >= pill_x + pill_w || local_y < pill_y"
        " || local_y >= pill_y + pill_h {",
        "        if rx < pill_x || rx >= pill_x + pill_w"
        " || local_y >= Self::app_card_top(idx) + 35.0 {",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the app card list has no bottom bound, so a clipped card is still clickable",
        "        if !local_y.is_finite() || local_y < 0.0 || local_y >= self.list_height() {",
        "        if !local_y.is_finite() || local_y < 0.0 {",
    ),
    (
        # The `is_finite` guard alone is not reintroducible -- the walk refuses
        # a NaN on its own, because a NaN fails both of its comparisons. What
        # the test is really holding down is the *shape*: the moment the walk
        # goes back to a division and a cast, `NaN as usize` is 0 and a
        # coordinate that is nowhere at all names the first app.
        "desktop",
        NOTIF_PANE,
        "the per-app walk goes back to dividing and casting",
        "        (0..self.app_settings.len()).find(|&idx| {\n"
        "            let top = Self::app_card_top(idx);\n"
        "            local_y >= top && local_y < top + APP_CARD_HEIGHT\n"
        "        })\n",
        "        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]\n"
        "        let idx = ((local_y - APP_HEADING_HEIGHT) / APP_CARD_PITCH) as usize;\n"
        "        (idx < self.app_settings.len()).then_some(idx)\n",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the per-app renderer drifts the pill three pixels below its own hit test",
        "            let (pill_x, pill_y, pill_w, pill_h) = Self::app_toggle_rect(y);",
        "            let (pill_x, pill_y, pill_w, pill_h) = Self::app_toggle_rect(y + 3.0);",
    ),
    (
        "desktop",
        NOTIF_PANE,
        "the per-app renderer walks a running total instead of asking app_card_top",
        "            let y = start_y + Self::app_card_top(idx);",
        "            let y = start_y + APP_HEADING_HEIGHT + (idx as f32) * (APP_CARD_PITCH + 1.0);",
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

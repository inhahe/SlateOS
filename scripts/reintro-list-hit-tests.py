"""Reintroduction check for the speedtest / compass list hit-test fixes.

Puts each fixed defect back one at a time and asserts that at least one test
fails. A defect that can be restored with every test still green is a defect
the suite does not actually pin -- which is exactly what "N tests pass" hides,
and exactly how the mirrored highlight below survived sixty-six of them.

The defects here are the same three families as `reintro-scroll-panes.py`,
found in two more apps:

1. The same layout arithmetic written twice -- once by the renderer, once by
   the hit test. In the speed test the renderer reversed the list so the
   newest result sat on top and the hit test did not, so the list was
   *mirrored*: pointing at the newest result highlighted the oldest.
2. A hit test that ignores the clip. The speed test's stats strip is opaque
   and painted over the foot of the list; the hit test ran to the bottom of
   the panel, so pointing at "avg 84.2 Mbps" selected a result underneath it.
3. A scroll offset that nothing bounds -- and here, nothing wrote at all:
   `history_scroll` had no wheel handler, so twelve of twenty results were
   drawn nowhere and reachable by nothing.

Plus the tripwire that lets the compass waypoint list get away with having no
scrolling: raising `MAX_WAYPOINTS` past what fits on screen must fail loudly
rather than pushing waypoints under the status bar.

Run from the lane-c worktree root:
    python scripts/reintro-list-hit-tests.py
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

SPEEDTEST = Path("apps/speedtest/src/main.rs")
COMPASS = Path("apps/compass/src/main.rs")

# (crate, path, name, old, new) -- `old` must occur exactly once in `path`.
DEFECTS = [
    # ------------------------------------------------------------- speedtest
    (
        "speedtest",
        SPEEDTEST,
        "the speed test's history is drawn newest-first and hit-tested oldest-first",
        "            .find(|&(ry, _)| y >= ry && y < ry + HISTORY_ROW_HEIGHT)\n"
        "            .map(|(_, idx)| idx)",
        "            .position(|(ry, _)| y >= ry && y < ry + HISTORY_ROW_HEIGHT)",
    ),
    (
        "speedtest",
        SPEEDTEST,
        "the history is drawn oldest-first, so the newest result is off the bottom",
        "        (0..self.history.len())\n"
        "            .rev()\n"
        "            .enumerate()",
        "        (0..self.history.len())\n"
        "            .enumerate()",
    ),
    (
        "speedtest",
        SPEEDTEST,
        "the history hit test runs under the opaque stats strip",
        "        let bottom = top + self.history_viewport_height();",
        "        let bottom = top + HISTORY_HEIGHT - HISTORY_HEADER_HEIGHT;",
    ),
    (
        "speedtest",
        SPEEDTEST,
        "the history scrolls one pixel per notch",
        "self.scroll_history_by(wheel::pixels(dy, HISTORY_ROW_HEIGHT));",
        "self.history_scroll -= dy;",
    ),
    (
        "speedtest",
        SPEEDTEST,
        "the history has no upper scroll bound",
        "        self.history_scroll = self.history_scroll.clamp(0.0, self.max_history_scroll());",
        "        self.history_scroll = self.history_scroll.max(0.0);",
    ),
    # --------------------------------------------------------------- compass
    (
        "compass",
        COMPASS,
        "a waypoint click lands one row below where it was aimed",
        "        let row = ((y - WP_LIST_TOP) / WP_ROW_HEIGHT) as usize;",
        "        let row = ((y - WP_LIST_TOP) / WP_ROW_HEIGHT) as usize + 1;",
    ),
    (
        "compass",
        COMPASS,
        "the waypoint click region is narrower than the row it is painted in",
        "        if x < WP_LIST_X || x > WP_LIST_X + WP_LIST_WIDTH || y < WP_LIST_TOP {",
        "        if x < 40.0 || x > 860.0 || y < WP_LIST_TOP {",
    ),
    (
        "compass",
        COMPASS,
        "more waypoints are allowed than fit in a list that cannot scroll",
        "const MAX_WAYPOINTS: usize = 10;",
        "const MAX_WAYPOINTS: usize = 100;",
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

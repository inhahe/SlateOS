#!/usr/bin/env python3
"""Print which of the three parallel-agent lanes *this* session owns.

Three Claude sessions work this repo simultaneously, one per Claude
account, and each owns a disjoint set of files (see roadmap.md ->
"Three-Agent Parallel Execution").  An agent must therefore be able to
answer "which one am I?" without asking the operator.

The discriminator is the ``CLAUDE_CONFIG_DIR`` environment variable,
which Claude Code sets to the per-account configuration directory.  The
operator keeps all three accounts under ``C:\\Users\\inhah\\.claude*``::

    C:\\Users\\inhah\\.claude              -> lane A   (the default account:
                                                      CLAUDE_CONFIG_DIR is
                                                      often *unset* for it)
    C:\\Users\\inhah\\.claude-account-b    -> lane B
    C:\\Users\\inhah\\.claude-account-c    -> lane C

Matching is on the directory's *suffix*, not its full path, so the tree
can be relocated or the accounts renamed to ``...-account-a`` without
touching this script.

Usage::

    python scripts/which-lane.py           # human-readable briefing
    python scripts/which-lane.py --letter  # just "A", "B" or "C"

Exit status: 0 when the lane was identified, 2 when it could not be
(an unrecognised ``CLAUDE_CONFIG_DIR``).  On 2, *stop and ask the
operator* rather than guessing — writing outside your lane is the one
failure mode that silently destroys another agent's work.
"""

from __future__ import annotations

import argparse
import os
import sys

# Lane letter -> (name, branch, owns, never writes).  Kept in sync with the
# ownership table in roadmap.md; that table is the authority.
LANES = {
    "A": (
        "Kernel & Core",
        "lane-a",
        "kernel/**, bench/**, toolchain/x86_64-slateos.json, "
        "scripts/boot-test.sh, scripts/run-timeout.py, scripts/wedge-soak.sh",
        "posix/**, userspace/**, gui/**, apps/**, net/**, services/**",
    ),
    "B": (
        "POSIX & Userland",
        "lane-b",
        "posix/**, userspace/**, services/**, init/**, toolchain/stubs/**, "
        "toolchain/build-sysroot.ps1, scripts/create-ext4-rootfs.sh",
        "kernel/**, gui/**, apps/**, net/**",
    ),
    "C": (
        "Graphics, Apps & Net",
        "lane-c",
        "gui/**, apps/**, net/**, netipc/**, netproto/**, netring/**, pkg/**",
        "kernel/**, posix/**, userspace/**, services/**",
    ),
}

# Config-directory suffix -> lane letter.  Checked longest-first so that
# "-account-b" wins over the bare ".claude" prefix it also ends with.
SUFFIX_TO_LANE = {
    "-account-a": "A",
    "-account-b": "B",
    "-account-c": "C",
    ".claude": "A",
}


def detect_lane() -> tuple[str | None, str]:
    """Return ``(lane_letter_or_None, config_dir_as_reported)``."""
    raw = os.environ.get("CLAUDE_CONFIG_DIR", "")

    # An unset variable means the default account, i.e. ~/.claude -> lane A.
    if not raw.strip():
        return "A", "<unset: default account ~/.claude>"

    # Normalise: strip quotes, whitespace and any trailing separator.  One of
    # the operator's directories genuinely has a trailing space in its name,
    # so do not assume the value is already clean.
    cleaned = raw.strip().strip('"').strip("'").rstrip()
    cleaned = cleaned.replace("\\", "/").rstrip("/")
    tail = cleaned.rsplit("/", 1)[-1].rstrip().lower()

    for suffix in sorted(SUFFIX_TO_LANE, key=len, reverse=True):
        if tail.endswith(suffix):
            return SUFFIX_TO_LANE[suffix], raw
    return None, raw


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Print which parallel-agent lane this session owns."
    )
    parser.add_argument(
        "--letter",
        action="store_true",
        help="print only the lane letter (A/B/C), for scripting",
    )
    args = parser.parse_args()

    lane, config_dir = detect_lane()

    if lane is None:
        print(
            "lane: UNKNOWN\n"
            f"CLAUDE_CONFIG_DIR: {config_dir!r}\n"
            "\n"
            "This directory matches none of the three known accounts. Do NOT\n"
            "guess a lane and do NOT edit shared files: ask the operator which\n"
            "lane this session is, then add the mapping to SUFFIX_TO_LANE in\n"
            "scripts/which-lane.py.",
            file=sys.stderr,
        )
        return 2

    if args.letter:
        print(lane)
        return 0

    name, branch, owns, never = LANES[lane]
    print(f"lane:              {lane}")
    print(f"name:              {name}")
    print(f"branch:            {branch}")
    print(f"CLAUDE_CONFIG_DIR: {config_dir}")
    print(f"owns (write):      {owns}")
    print(f"never writes:      {never}")
    print()
    print(
        "Work only items tagged `[%s]` in roadmap.md. Need a change in another\n"
        "lane's tree? File requests/%s-to-<lane>-<slug>.md instead of editing it.\n"
        "Full rules: roadmap.md -> \"Three-Agent Parallel Execution\"."
        % (lane, lane.lower())
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

# Locate the worktree the sourcing script physically lives in.
#
# Source this instead of typing a repo path into a script:
#
#     . "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh"      # scripts/x/y.sh
#     . "$(dirname "${BASH_SOURCE[0]}")/lib/worktree.sh"         # scripts/y.sh
#
# It exports:
#
#     SLATE_ROOT      the worktree root, as the running shell sees it
#     SLATE_LANE      its directory name: os, os-lane-a, os-lane-b, os-lane-c
#     SLATE_SYSROOT   $SLATE_ROOT/toolchain/sysroot/lib
#     SLATE_SPIKE     $SLATE_ROOT/build/spike   (created if absent)
#     SLATE_ROOTFS    $SLATE_ROOT/rootfs.ext4   (may not exist yet)
#     SLATE_ZIG       $SLATE_SPIKE/zig/zig      (may not exist yet)
#     SLATE_TMP       /tmp/slate-$SLATE_LANE    scratch, created; see below
#
# ---------------------------------------------------------------------------
# Why this file exists
# ---------------------------------------------------------------------------
# There are four checkouts of this repository — `os` plus one worktree per lane
# — and until 2026-08-16 nine scripts had `/mnt/d/visual studio projects/os/…`
# typed into them. In a multi-worktree repo a hard-coded path is not merely
# unportable, it is a **silent cross-lane write**: whichever lane ran the
# script read `os`'s libraries and wrote `os`'s output directory, so the
# artifact proved nothing about the tree it ran in, the other tree's copy was
# clobbered without its knowledge, and the invoking tree ended up with nothing.
#
# That is not a hypothetical. `scripts/bash-spike/slatelink.sh` had exactly
# this bug, and the consequence was that lanes B and C never once executed the
# GNU bash we ship (design-decisions.md §305) — their boot tests SKIPped the
# `self_test_bash_on_slateos_libc` rung for four days while reporting PASSED.
# See known-issues.md →
# `B-THE-BASH-RELINK-SCRIPT-HARD-CODED-ONE-WORKTREE-SO-ONLY-main-EVER-RAN-BASH`.
#
# One helper rather than the same three lines pasted into every script: a rule
# replicated per file is a rule the next file opts out of by not having it,
# which is the lesson `services/.gitignore` was consolidated for on the same
# day.
# ---------------------------------------------------------------------------

# `${BASH_SOURCE[0]}` here is *this* file, wherever it was sourced from, so the
# root is two levels up from `scripts/lib/` regardless of the caller's depth.
SLATE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)" || return 1
SLATE_LANE="$(basename "$SLATE_ROOT")"
SLATE_SYSROOT="$SLATE_ROOT/toolchain/sysroot/lib"
SLATE_SPIKE="$SLATE_ROOT/build/spike"
SLATE_ROOTFS="$SLATE_ROOT/rootfs.ext4"
SLATE_ZIG="$SLATE_SPIKE/zig/zig"

# Scratch, keyed by worktree. The hard-coded paths were only half the problem:
# these scripts also wrote fixed names like /tmp/libc_syms.txt and
# /tmp/bash_needs.txt, and they hand results to each other through those files
# (syms.sh writes what quality.sh reads). Two lanes running concurrently would
# silently mix one lane's symbol tables into the other's analysis — and the
# result still looks like a clean report, which is the failure mode this repo
# keeps rediscovering. Lane-keyed scratch makes the mixing impossible rather
# than unlikely.
SLATE_TMP="/tmp/slate-$SLATE_LANE"

# A sanity check, not decoration: if the derived root is wrong, every path
# below it is wrong in the same direction, and the failure would otherwise
# surface as a confusing "no such file" several steps later.
if [ ! -f "$SLATE_ROOT/CLAUDE.md" ] || [ ! -d "$SLATE_ROOT/kernel" ]; then
    echo "worktree.sh: derived SLATE_ROOT=$SLATE_ROOT does not look like a" >&2
    echo "             SlateOS checkout (no CLAUDE.md / no kernel/)." >&2
    return 1
fi

mkdir -p "$SLATE_SPIKE" "$SLATE_TMP" 2>/dev/null || true

# `$CC` must not contain spaces: autotools word-splits it, and this repo lives
# under "D:\visual studio projects\", so pointing CC straight at a path under
# /mnt/d produces a thoroughly misleading "C compiler cannot create
# executables". These wrappers keep the space out of $CC. They are keyed by
# lane so two lanes configuring at once cannot hand each other's zig to
# autotools.
slate_make_zig_wrappers() {
    if [ ! -x "$SLATE_ZIG" ]; then
        echo "worktree.sh: no zig at $SLATE_ZIG" >&2
        echo "             The spikes need it; see scripts/bash-spike/README." >&2
        return 1
    fi
    SLATE_CC="/tmp/zigcc-$SLATE_LANE"
    SLATE_AR="/tmp/zigar-$SLATE_LANE"
    SLATE_RANLIB="/tmp/zigranlib-$SLATE_LANE"
    printf '#!/bin/sh\nexec "%s" cc --target=x86_64-linux-musl "$@"\n' "$SLATE_ZIG" >"$SLATE_CC"
    printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$SLATE_ZIG" >"$SLATE_AR"
    printf '#!/bin/sh\nexec "%s" ranlib "$@"\n' "$SLATE_ZIG" >"$SLATE_RANLIB"
    chmod +x "$SLATE_CC" "$SLATE_AR" "$SLATE_RANLIB"
}

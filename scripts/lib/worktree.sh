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
#     SLATE_ZIG       the pinned zig cross-toolchain; call slate_ensure_zig first
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

# The zig cross-toolchain. Pinned by version *and* hash: `zig cc
# --target=x86_64-linux-musl` is what compiles the objects that get linked
# against our libc.a, so a different zig is a different artifact, and
# `bash-slateos.elf` is a shipped product (design-decisions.md §305) rather than
# a scratch build.
#
# Unlike everything else in build/spike/, zig is *input* and not this tree's
# output, so — and only so — it is shared between worktrees rather than copied
# per lane. The cross-lane hazard this file exists to prevent is a lane reading
# another lane's *build products* (see below); a pinned third-party tarball has
# identical bytes for every lane by construction, so sharing it cannot make one
# lane's artifact depend on another lane's source.
SLATE_ZIG_VERSION="0.13.0"
SLATE_ZIG_SHA256="d45312e61ebcc48032b77bc4cf7fd6915c11fa16e4aad116b66c9468211230ea"
SLATE_ZIG_CACHE="${SLATE_ZIG_CACHE:-$HOME/.cache/slateos}"
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
# Resolve $SLATE_ZIG, downloading the pinned toolchain if this machine has not
# got it yet.
#
# Until 2026-08-16 there was no such step: zig had been placed by hand in
# *one* worktree's build/spike/, that directory is gitignored, and no script or
# document recorded the version or the URL. So `scripts/bash-spike/` — which
# §305 calls "the build recipe for a shipped artifact" and asks us to keep
# working — could not be run in three of the four checkouts, and could not be
# run at all in a fresh clone. The failure was silent in the way this repo keeps
# rediscovering: the recipe looked fine because the one tree anybody tested it
# in happened to have the missing piece lying around.
slate_ensure_zig() {
    # 1. A per-worktree copy, which is how the original spikes were provisioned.
    #    Preferred when present so existing checkouts neither re-download nor
    #    silently switch toolchain underneath a half-finished build tree.
    if [ -x "$SLATE_SPIKE/zig/zig" ]; then
        SLATE_ZIG="$SLATE_SPIKE/zig/zig"
        return 0
    fi

    local dir="$SLATE_ZIG_CACHE/zig-linux-x86_64-$SLATE_ZIG_VERSION"
    if [ -x "$dir/zig" ]; then
        SLATE_ZIG="$dir/zig"
        return 0
    fi

    local url="https://ziglang.org/download/$SLATE_ZIG_VERSION/zig-linux-x86_64-$SLATE_ZIG_VERSION.tar.xz"
    local tarball="$SLATE_ZIG_CACHE/zig-linux-x86_64-$SLATE_ZIG_VERSION.tar.xz"
    echo "worktree.sh: zig $SLATE_ZIG_VERSION not found; fetching to $SLATE_ZIG_CACHE" >&2
    mkdir -p "$SLATE_ZIG_CACHE" || return 1
    if [ ! -f "$tarball" ]; then
        curl -sSL --fail --max-time 900 -o "$tarball.part" "$url" || {
            echo "worktree.sh: download failed: $url" >&2
            rm -f "$tarball.part"
            return 1
        }
        mv "$tarball.part" "$tarball"
    fi

    # Verify before extracting, not after: an unverified archive is executed by
    # the very next step, and this one produces a binary we ship.
    local got
    got="$(sha256sum "$tarball" | cut -d' ' -f1)"
    if [ "$got" != "$SLATE_ZIG_SHA256" ]; then
        echo "worktree.sh: zig tarball sha256 mismatch — refusing to extract." >&2
        echo "             expected $SLATE_ZIG_SHA256" >&2
        echo "             got      $got" >&2
        echo "             ($tarball — delete it to retry the download)" >&2
        return 1
    fi

    tar -xf "$tarball" -C "$SLATE_ZIG_CACHE" || return 1
    if [ ! -x "$dir/zig" ]; then
        echo "worktree.sh: extracted $tarball but $dir/zig is missing" >&2
        return 1
    fi
    SLATE_ZIG="$dir/zig"
    echo "worktree.sh: zig $SLATE_ZIG_VERSION ready at $SLATE_ZIG" >&2
}

slate_make_zig_wrappers() {
    slate_ensure_zig || return 1
    if [ ! -x "$SLATE_ZIG" ]; then
        echo "worktree.sh: no zig at $SLATE_ZIG" >&2
        echo "             The spikes need it; see scripts/bash-spike/README.md." >&2
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

#!/bin/bash
# Prove the Python 3 spec harness judges every case as upstream's does.
#
# It runs scripts/oils-spec/sh_spec.py and upstream's own harness over every
# spec file of the release, against the same Oils, and requires the two
# results tables to be identical.
#
# WHY THIS EXISTS. sh_spec.py is a Python 3 port of Oils' Python 2 harness,
# made so the spec tests can run on SlateOS (design-decisions.md §1043). A port
# that judged a case differently would report a SlateOS difference that is
# really a harness difference -- or hide one. So it is held to upstream's
# verdicts case by case, here on Linux where both can run: upstream's harness
# under Python 2 with its own Python 2 helpers, the port under Python 3 with
# the ports of those helpers (scripts/oils-spec/bin), both against one native
# build of the same Oils release, with the same PATH, $TMP and environment.
# Any cell that differs is a defect in the port or in a helper.
#
# NEEDS: WSL with a C++ compiler (the native Oils build), curl, python3, and a
# Python 2.7 for upstream's harness -- PY2=/path/to/python2, or by default
# /tmp/py27/bin/python2, which conda makes without root:
#     ~/miniconda3/bin/conda create -p /tmp/py27 -c conda-forge python=2.7
#
# usage: bash scripts/oils-spec/validate.sh [SPEC_NAME...]
#   With no names, every spec/*.test.sh. Results in $SLATE_WORK/oils-spec-validate.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="0.38.0"
TARBALL_SHA256="a33453722819b55ee552bfd7f3c2bab8f1940def55d5c8b46af16ce95bdf8803"
# The commit at the head of upstream's release/0.38.0 branch, whose archive
# carries the spec files and the harness that the release tarball does not.
SRC_COMMIT="a681da6c12a1f6280539e2e2a068720ace54db07"
SRC_SHA256="f9b02495b7c734f91c56659544641281b108319e28518bb1a4f069ac0d59d024"
PY2="${PY2:-/tmp/py27/bin/python2}"
TIMEOUT=20

WORK="$SLATE_WORK/oils-spec-validate"
HERE="$SLATE_ROOT/scripts/oils-spec"
mkdir -p "$WORK" || exit 1
cd "$WORK" || exit 1

fetch() {  # url file sha256
    if [ ! -f "$2" ] || ! echo "$3  $2" | sha256sum -c --quiet 2>/dev/null; then
        curl -sSfL -o "$2.part" "$1" || { echo "NO_DOWNLOAD -- $1"; return 1; }
        mv "$2.part" "$2"
    fi
    echo "$3  $2" | sha256sum -c --quiet || { echo "HASH_MISMATCH -- $2"; return 1; }
}

if [ ! -x "$PY2" ]; then
    echo "NO_PYTHON2 -- $PY2; see the header for the one-line conda command"
    exit 1
fi

fetch "https://oils.pub/download/oils-for-unix-$VER.tar.gz" "oils-for-unix-$VER.tar.gz" "$TARBALL_SHA256" || exit 1
fetch "https://github.com/oils-for-unix/oils/archive/$SRC_COMMIT.tar.gz" "oils-src-$SRC_COMMIT.tar.gz" "$SRC_SHA256" || exit 1

# The shell both harnesses run: the release built natively.
NATIVE="$WORK/oils-for-unix-$VER/_bin/cxx-opt-sh"
if [ ! -x "$NATIVE/osh" ]; then
    rm -rf "oils-for-unix-$VER"
    tar xzf "oils-for-unix-$VER.tar.gz" || exit 1
    (cd "oils-for-unix-$VER" && ./configure --without-readline >configure.log 2>&1 \
        && _build/oils.sh --without-readline >build.log 2>&1) \
        || { echo "NATIVE_BUILD_FAILED -- see $WORK/oils-for-unix-$VER/build.log"; exit 1; }
fi

SRC="$WORK/oils-src"
if [ ! -f "$SRC/test/sh_spec.py" ]; then
    rm -rf "$SRC" && mkdir -p "$SRC"
    tar xzf "oils-src-$SRC_COMMIT.tar.gz" -C "$SRC" --strip-components=1 || exit 1
fi

# One helper directory, at one path, holding upstream's helpers for the
# reference run and the ports for the port's: PATH is then the same string in
# both runs, and so is anything a case prints from it.
HBIN="$WORK/helpers"
TMPENV="$WORK/tmp"
PATHENV="$HBIN:$(dirname "$PY2"):$PATH"

use_helpers() {  # directory to copy them from
    rm -rf "$HBIN" && mkdir -p "$HBIN" && cp -p "$1"/* "$HBIN"/ && chmod +x "$HBIN"/*
}

run_harness() {  # which(ref|port) spec_file tsv_out log_out
    local common=(--tmp-env "$TMPENV" --path-env "$PATHENV"
        --env-pair LC_ALL=C.UTF-8 --env-pair "REPO_ROOT=$SRC"
        --timeout "$TIMEOUT" --oils-bin-dir "$NATIVE" --tsv-output "$3")
    if [ "$1" = ref ]; then
        (cd "$SRC" && PYTHONPATH=. "$PY2" test/sh_spec.py "${common[@]}" "$2") >"$4" 2>&1
    else
        (cd "$SRC" && python3 "$HERE/sh_spec.py" "${common[@]}" "$2") >"$4" 2>&1
    fi
}

if [ $# -gt 0 ]; then
    FILES=()
    for name in "$@"; do FILES+=("$SRC/spec/$name.test.sh"); done
else
    FILES=("$SRC"/spec/*.test.sh)
fi

mkdir -p ref port
rm -f ref/*.tsv port/*.tsv
# Upstream's helpers keep spec/bin's two non-Python files; the ports replace
# the five Python ones and nothing else, so copy upstream's set first.
use_helpers "$SRC/spec/bin"
for f in "${FILES[@]}"; do
    n="$(basename "$f" .test.sh)"
    run_harness ref "$f" "$WORK/ref/$n.tsv" "$WORK/ref/$n.log"
    echo "$?" >"ref/$n.rc"
done
use_helpers "$SRC/spec/bin"
cp -p "$HERE"/bin/*.py "$HBIN"/ && chmod +x "$HBIN"/*
for f in "${FILES[@]}"; do
    n="$(basename "$f" .test.sh)"
    run_harness port "$f" "$WORK/port/$n.tsv" "$WORK/port/$n.log"
    echo "$?" >"port/$n.rc"
done

SAME=0
DIFF=0
: >differences.txt
for f in "${FILES[@]}"; do
    n="$(basename "$f" .test.sh)"
    if [ ! -f "ref/$n.tsv" ]; then
        echo "$n: the reference harness wrote no table (see ref/$n.log)" >>differences.txt
        DIFF=$((DIFF + 1))
        continue
    fi
    if cmp -s "ref/$n.tsv" "port/$n.tsv" && cmp -s "ref/$n.rc" "port/$n.rc"; then
        SAME=$((SAME + 1))
    else
        DIFF=$((DIFF + 1))
        {
            echo "== $n (exit: ref $(cat "ref/$n.rc"), port $(cat "port/$n.rc" 2>/dev/null))"
            diff "ref/$n.tsv" "port/$n.tsv" | head -20
        } >>differences.txt
    fi
done
CASES="$(cat ref/*.tsv | grep -vc '^case')"
echo "SPEC_FILES=${#FILES[@]} CASES=$CASES IDENTICAL=$SAME DIFFERENT=$DIFF"
head -60 differences.txt
[ "$DIFF" -eq 0 ]

#!/bin/bash
# Build the tree that puts Oils' spec tests on a SlateOS image.
#
#   build/oils-spec/usr/share/oils-spec/
#       sh_spec.py, run_all.py     the harness (a Python 3 port) and its driver
#       LICENSE.txt                Oils' licence: the spec files below are upstream's
#       spec/*.test.sh             every spec file of the release
#       spec/testdata/             what the cases read through $REPO_ROOT
#       spec/bin/                  the helpers cases run: upstream's two shell
#                                  files, and the Python 3 ports of its five
#                                  Python 2 ones (scripts/oils-spec/bin)
#       expected/*.tsv             the same run recorded on Linux (run_all.py
#                                  --record), and expected/unstable.tsv, the
#                                  cells three Linux runs did not all agree on
#
# On SlateOS: python3 /usr/share/oils-spec/run_all.py -- it runs every file
# against /bin/oils-for-unix and reports each cell that differs from Linux.
# design-decisions.md §1043 asks for exactly this before genuine Oils becomes
# the default shell.
#
# The spec files come from the same pinned commit validate.sh uses, so the
# expectations and the cases cannot come from different revisions.
#
# Run it from WSL. usage: bash scripts/oils-spec/bundle.sh [--out DIR]
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SRC_COMMIT="a681da6c12a1f6280539e2e2a068720ace54db07"
SRC_SHA256="f9b02495b7c734f91c56659544641281b108319e28518bb1a4f069ac0d59d024"
HERE="$SLATE_ROOT/scripts/oils-spec"
OUT="$SLATE_ROOT/build/oils-spec"
if [ "${1:-}" = "--out" ] && [ -n "${2:-}" ]; then
    OUT="$2"
fi
# The release built natively, as validate.sh builds it; OILS_NATIVE names
# another.
NATIVE="${OILS_NATIVE:-$SLATE_WORK/oils-spec-validate/oils-for-unix-0.38.0/_bin/cxx-opt-sh/oils-for-unix}"
ARCHIVE="$SLATE_WORK/oils-src-$SRC_COMMIT.tar.gz"
DEST="$OUT/usr/share/oils-spec"

if [ ! -f "$ARCHIVE" ] || ! echo "$SRC_SHA256  $ARCHIVE" | sha256sum -c --quiet 2>/dev/null; then
    curl -sSfL -o "$ARCHIVE.part" "https://github.com/oils-for-unix/oils/archive/$SRC_COMMIT.tar.gz" \
        || { echo "NO_DOWNLOAD"; exit 1; }
    mv "$ARCHIVE.part" "$ARCHIVE"
fi
echo "$SRC_SHA256  $ARCHIVE" | sha256sum -c --quiet || { echo "HASH_MISMATCH -- $ARCHIVE"; exit 1; }

# Regenerable, and stale files would be staged: start empty -- but only a
# tree this script made (it has run_all.py) or a directory that is not there.
if [ -e "$OUT" ] && [ ! -f "$DEST/run_all.py" ]; then
    echo "REFUSING -- $OUT exists and is not a tree this script made"
    exit 1
fi
rm -rf "$OUT"
mkdir -p "$DEST" || exit 1

STAGE="$(mktemp -d)" || exit 1
trap 'rm -rf "$STAGE"' EXIT
tar xzf "$ARCHIVE" -C "$STAGE" --strip-components=1 \
    --wildcards '*/spec/*.test.sh' '*/spec/testdata/*' '*/spec/bin/*' '*/LICENSE.txt' \
    || { echo "EXTRACT_FAILED"; exit 1; }

cp "$HERE/sh_spec.py" "$HERE/run_all.py" "$DEST/" || exit 1
cp "$STAGE/LICENSE.txt" "$DEST/" || exit 1
mkdir -p "$DEST/spec/bin" || exit 1
cp "$STAGE"/spec/*.test.sh "$DEST/spec/" || exit 1
cp -r "$STAGE/spec/testdata" "$DEST/spec/" || exit 1
# Upstream's helpers that are not Python, then the ports of those that are.
for f in "$STAGE"/spec/bin/*; do
    case "$f" in
        *.py) ;;
        *) cp -p "$f" "$DEST/spec/bin/" ;;
    esac
done
cp "$HERE"/bin/*.py "$DEST/spec/bin/" || exit 1
chmod 0755 "$DEST"/spec/bin/* "$DEST/sh_spec.py" "$DEST/run_all.py"

UPSTREAM_PY="$(cd "$STAGE/spec/bin" && ls ./*.py | sed 's|^\./||' | sort)"
PORTED_PY="$(cd "$HERE/bin" && ls ./*.py | sed 's|^\./||' | sort)"
if [ "$UPSTREAM_PY" != "$PORTED_PY" ]; then
    echo "HELPERS_DIFFER -- upstream's Python helpers are not the ones ported:"
    echo "upstream: $UPSTREAM_PY"
    echo "ported:   $PORTED_PY"
    exit 1
fi

# The expectations: this tree's own driver, run on Linux against the release
# built natively -- the same scripts, PATH shape and paths it will have on
# SlateOS, so the system is the only thing that differs (run_all.py's
# docstring has the 85 cells that taught this). Three times, so cells the
# Linux runs do not all agree on are set aside as unstable.
#
# In /tmp/oils-spec, the path the run on SlateOS uses: cases run in
# directories under it and some see it (run_all.py, --out). /tmp is shared by
# every lane's WSL, hence the lock.
#
# PYTHONDONTWRITEBYTECODE: the driver imports sh_spec.py from the tree, and
# Python would otherwise leave __pycache__/sh_spec.cpython-312.pyc in it --
# compiled by this host's Python, shipped to the image with everything else.
if [ -x "$NATIVE" ]; then
    PYTHONDONTWRITEBYTECODE=1 flock /tmp/oils-spec.lock \
        python3 "$DEST/run_all.py" --record --oils-for-unix "$NATIVE" --out /tmp/oils-spec \
        >"$SLATE_WORK/oils-spec-record.log" 2>&1
    echo "RECORD_EXIT=$?"
    grep OILS_SPEC_RECORDED "$SLATE_WORK/oils-spec-record.log"
else
    echo "EXPECTED_TABLES=0 -- no native Oils at $NATIVE; scripts/oils-spec/validate.sh builds it"
fi

echo "SPEC_FILES=$(ls "$DEST"/spec/*.test.sh | wc -l)"
echo "BUNDLE_BYTES=$(du -sb "$OUT" | cut -f1)"
echo "OILS_SPEC_BUNDLE $OUT"

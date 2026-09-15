#!/bin/bash
# Measure GNU diff's -y (side-by-side) layout.
#
# Ours differs from GNU in two ways that the harness shows together but which
# are separate rules: GNU pairs a changed line onto ONE line with `|`, where we
# emit a `<` line and a `>` line; and GNU pads with TABS where we pad with
# spaces. Both are layout rules that must be measured rather than guessed --
# the gutter column, the pairing when the counts are uneven, and where the tab
# stops fall are all things a plausible implementation gets wrong.
#
# `cat -A` throughout: the whole subject is invisible characters. `^I` is a tab
# and `$` is end of line, so padding is readable instead of guessed at.
set -u
DIFF=/usr/bin/diff
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
cd "$tmp" || exit 1

show() {
    local label="$1"; shift
    echo "--- $label ---"
    "$DIFF" "$@" 2>&1 | cat -A
    echo "   (rc=$?)"
}

echo "=== GNU $($DIFF --version | head -1) ==="

# One line changed: the case the harness covers.
printf 'alpha\nbravo\ncharlie\ndelta\n' > a
printf 'alpha\nbravo\nCHANGED\ndelta\n' > b
show 'one change' -y a b

# Uneven counts: two removed against one added. Does it pair the first and
# leave the second as `<`, or emit all deletes then all inserts?
printf 'k\nx1\nx2\nz\n' > c
printf 'k\ny1\nz\n' > d
show 'two out, one in' -y c d
show 'one out, two in' -y d c

# Pure delete and pure insert, to see the gutter for each.
printf 'k\ngone\nz\n' > e
printf 'k\nz\n' > f
show 'pure delete' -y e f
show 'pure insert' -y f e

# A line long enough to be truncated, and the default width.
printf 'k\n%s\nz\n' "$(printf 'L%.0s' $(seq 1 100))" > g
printf 'k\nshort\nz\n' > h
show 'long line, default width' -y g h
show 'long line, -W 40' -y -W 40 g h

# --expand-tabs, which is the other half of the tab question.
show 'one change, -t' -y -t a b
show 'suppress common' -y --suppress-common-lines a b

echo "=== done ==="

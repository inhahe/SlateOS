#!/usr/bin/env bash
# mktemp-diff.sh — compare our `mktemp` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# Each case is one shell command line; standard output and standard error go
# to one file, compared byte for byte with the exit status after them.
#
# A name `mktemp` makes is random, so the cases turn the random characters
# back into X's before comparing -- `xs N SUFFIX` does it for the last N
# alphanumerics before SUFFIX -- and compare what was made instead: whether a
# file or a directory, its permissions, whether anything was made at all
# (`-u`), and whether a name that could not be printed took its file with it.
# The rest is where the name goes (`-p`, `--tmpdir`, `-t` and `$TMPDIR`, which
# rank differently for `-t`), how the X's and the suffix are found, and every
# message on the way.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block, and `--version` and its undocumented
# alias `-V` name SlateOS.
set -u

DIFF_PROG='mktemp'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

for v in $(env | sed -n 's/^\(LC_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
export LC_ALL=C.UTF-8
unset TMPDIR

pass=0; fail=0; xfail=0; xpass=0

GNU_PATH="$bindir/gnu:$PATH"
OURS_PATH="$bindir/ours:$PATH"
out_a="$DIFF_TMP/out-gnu"
out_b="$DIFF_TMP/out-ours"

mkdir -p "$DIFF_TMP/tree" || exit 1
cd "$DIFF_TMP/tree" || exit 1
tree=$PWD

# ---------------------------------------------------------------- fixtures ---

mkdir d e tdir
mkdir locked && chmod 000 locked

mktemp() { PATH=$MKTEMP_PATH command mktemp "$@"; }
# The last N alphanumerics before SUFFIX (a regex), back into X's; and the
# tree's path, as `TREE`.
xs() {
    local n=$1 suffix=${2:-} x
    x=$(printf 'X%.0s' $(seq "$n"))
    sed -E "s|$tree|TREE|; s/[A-Za-z0-9]{$n}($suffix)\$/$x\1/"
}
# How many entries the tree holds, so a case can see whether one was made.
count() { find "$tree" -mindepth 1 2>/dev/null | wc -l; }

# ------------------------------------------------------------------ driver ---

run_case() {
    line=$1
    expect_diff=0
    reason=""
    case $line in
        '!'*)
            expect_diff=1
            reason=${line#!}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
    esac
    ( MKTEMP_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( MKTEMP_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )
    if cmp -s "$out_a" "$out_b"; then
        if [ "$expect_diff" = 1 ]; then
            xpass=$((xpass + 1))
            printf 'XPASS  %s\n     (expected to differ: %s)\n' "$line" "$reason"
        else
            pass=$((pass + 1))
            [ -n "${VERBOSE:-}" ] && printf 'OK     %s\n' "$line"
        fi
        return
    fi
    if [ "$expect_diff" = 1 ]; then
        xfail=$((xfail + 1))
        return
    fi
    fail=$((fail + 1))
    printf 'FAIL   %s\n' "$line"
    printf '  ---- unified (gnu < , ours >) ----\n'
    diff <(cat -A "$out_a") <(cat -A "$out_b") \
        | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- a file, a directory, a name ---
mktemp fooXXXXXX | xs 6
f=$(mktemp fooXXXX) && stat -c '%a %F %s' "$f"
d=$(mktemp -d dirXXX) && stat -c '%a %F' "$d"
f=$(mktemp --directory dXXXXX) && test -d "$f" && echo dir
umask 077; f=$(mktemp uXXX) && stat -c '%a' "$f"
umask 022; d=$(mktemp -d uXXX) && stat -c '%a' "$d"
n=$(mktemp -u dryXXXX) && { test -e "$n" && echo made || echo free; } && echo "$n" | xs 4
n=$(mktemp --dry-run -d dryXXXX) && { test -e "$n" && echo made || echo free; }

# --- the X's and the suffix ---
mktemp fooXXXbar | xs 3 bar
mktemp fooXXXbar.txt | xs 3 'bar\.txt'
mktemp aXXXXXXXXXXXXXXXXXXXX | xs 20
mktemp --suffix=.txt aXXX | xs 3 '\.txt'
mktemp --suffix= aXXX | xs 3
mktemp --suffix=.txt aXXXb
mktemp --suffix=.txt ''
mktemp --suffix=/x aXXX
mktemp fooXX
mktemp fooXXbarX
mktemp XXX | xs 3
mktemp abc
mktemp 'with space XXX' | xs 3
mktemp aXXX/bXXX

# --- where the name goes ---
TMPDIR=$PWD/tdir mktemp | xs 10
TMPDIR= mktemp | sed -E 's/[A-Za-z0-9]{10}$/XXXXXXXXXX/'
mktemp -p d aXXX | xs 3
mktemp -p d/ aXXX | xs 3
mktemp -p "$PWD/d" aXXX | xs 3
mktemp --tmpdir=d aXXX | xs 3
TMPDIR=$PWD/e mktemp --tmpdir aXXX | xs 3
TMPDIR=$PWD/e mktemp -p d aXXX | xs 3
TMPDIR=$PWD/e mktemp -t aXXX | xs 3
TMPDIR=$PWD/e mktemp -t -p d aXXX | xs 3
TMPDIR= mktemp -t -p d aXXX | xs 3
TMPDIR=$PWD/e mktemp -p '' aXXX | xs 3
mktemp -p d a/bXXX
mkdir -p d/a && mktemp -p d a/bXXX | xs 3
mktemp -t a/bXXX
mktemp -t /aXXX
mktemp -p d /aXXX
mktemp -p / tmpXXX >/dev/null; echo $?

# --- what cannot be made ---
mktemp -p nonexistent aXXX
mktemp -d -p nonexistent aXXX
mktemp -q -p nonexistent aXXX
mktemp -u -p nonexistent aXXX | xs 3
mktemp -p locked aXXX
mktemp -u -p locked aXXX
mktemp -q -d -p locked aXXX

# --- a name that cannot be printed takes its file with it ---
before=$(count); mktemp gone.XXXX >/dev/full; echo "rc=$? made=$(( $(count) - before ))"
before=$(count); mktemp -d gone.XXXX >/dev/full; echo "rc=$? made=$(( $(count) - before ))"
before=$(count); mktemp -q gone.XXXX >/dev/full; echo "rc=$? made=$(( $(count) - before ))"
mktemp -u gone.XXXX >/dev/full

# --- the command line ---
mktemp a b
mktemp -Z
mktemp --bogus
mktemp -p
mktemp --suffix
mktemp --dir dXXX >/dev/null; echo $?
mktemp --tmp=d aXXX | xs 3

# --- deliberately different ---
!--help text is ours|mktemp --help
!--version text is ours|mktemp --version
!-V is --version, whose text is ours|mktemp -V
CASES

chmod 700 locked

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

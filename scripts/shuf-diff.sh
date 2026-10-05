#!/usr/bin/env bash
# shuf-diff.sh — compare our `shuf` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# Each case is one shell command line; standard output and standard error go
# to one file, compared byte for byte with the exit status after them.
#
# A shuffle is random, so nearly every case reads its randomness from a file
# (`--random-source=rnd`, bytes generated from a fixed seed): two programs that
# consume the bytes the same way -- gnulib's `randint`, its `randperm`, and the
# reservoir that `-n` keeps when the input's size is unknown -- print the same
# permutation, and two that do not, do not. The cases reach each path that
# draws differently: a whole input permuted; a head count; `-e` and `-i`; a
# range large and sparse enough for gnulib's hash-table permutation, and one
# that is not; `-r`, which draws per line; a pipe with `-n` (the reservoir, one
# draw per line past the head and one more); an input over 8 MiB with `-n`,
# which samples a reservoir from a file too; and a random source too short to
# finish. The few cases without `--random-source` compare what does not depend
# on the draw: the lines, sorted.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='shuf'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

for v in $(env | sed -n 's/^\(LC_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

GNU_PATH="$bindir/gnu:$PATH"
OURS_PATH="$bindir/ours:$PATH"
out_a="$DIFF_TMP/out-gnu"
out_b="$DIFF_TMP/out-ours"

mkdir -p "$DIFF_TMP/tree" || exit 1
cd "$DIFF_TMP/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---

# The random source: 1 MiB of bytes from a fixed seed, so a failure reruns.
awk 'BEGIN { srand(11); for (i = 0; i < 1048576; i++) printf "%c", int(rand() * 256) }' > rnd
head -c 3 rnd > short
for i in $(seq 1 10); do printf 'line %d\n' "$i"; done > lines
printf 'one\ntwo\nthree' > noeol
printf 'only\n' > single
: > empty
printf 'a\0b\0c\0d\0' > nuls
printf 'x\0y' > nulnoeol
seq 1 1000 > thousand
# Over 8 MiB, so `-n` samples a reservoir from a file as well as a pipe.
seq 1 1300000 > big
cp lines inplace
mkdir dir

shuf() { PATH=$SHUF_PATH command shuf "$@"; }

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
    cp lines inplace
    rm -f out
    ( SHUF_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    cp lines inplace
    rm -f out
    ( SHUF_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )
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
# --- a whole input ---
shuf --random-source=rnd lines
shuf --random-source=rnd noeol
shuf --random-source=rnd single
shuf --random-source=rnd empty
shuf --random-source=rnd thousand | md5sum
shuf --random-source=rnd - < lines
shuf --random-source=rnd < lines
cat lines | shuf --random-source=rnd
shuf --random-source=rnd -z nuls | od -c
shuf --random-source=rnd -z nulnoeol | od -c

# --- a head count ---
shuf --random-source=rnd -n 3 lines
shuf --random-source=rnd -n 0 lines
shuf --random-source=rnd -n 0 nonexistent
shuf --random-source=rnd -n 1 lines
shuf --random-source=rnd -n 100 lines
shuf --random-source=rnd --head-count=4 thousand
shuf --random-source=rnd -n 5 -n 3 lines
shuf --random-source=rnd -n 99999999999999999999 lines

# --- the reservoir: a pipe, or an input over 8 MiB ---
cat lines | shuf --random-source=rnd -n 3
cat lines | shuf --random-source=rnd -n 1
cat lines | shuf --random-source=rnd -n 100
cat thousand | shuf --random-source=rnd -n 7
cat noeol | shuf --random-source=rnd -n 2
shuf --random-source=rnd -n 5 big
shuf --random-source=rnd -n 5 < big
printf 'a\0b\0c' | shuf --random-source=rnd -z -n 2 | od -c

# --- -e ---
shuf --random-source=rnd -e a b c d e
shuf --random-source=rnd -e
shuf --random-source=rnd -n 2 -e a b c
shuf --random-source=rnd -e 'two words' x
shuf --random-source=rnd -z -e a b c | od -c

# --- -i ---
shuf --random-source=rnd -i 1-10
shuf --random-source=rnd -i 1-10 -n 3
shuf --random-source=rnd -i 5-4
shuf --random-source=rnd -i 0-0
shuf --random-source=rnd -i 7-7
shuf --random-source=rnd -i 1-1000000 -n 10
shuf --random-source=rnd -i 1-1000000 -n 31250 | md5sum
shuf --random-source=rnd -i 1-1000000 -n 31251 | md5sum
shuf --random-source=rnd -i 1-131071 -n 10
shuf --random-source=rnd -i 1-200000 -n 5000 | md5sum
shuf --random-source=rnd -i 18446744073709551610-18446744073709551614
shuf --random-source=rnd --input-range=3-5

# --- -r ---
shuf --random-source=rnd -r -n 10 lines
shuf --random-source=rnd -r -n 10 -i 1-3
shuf --random-source=rnd -r -n 5 -e x y
shuf --random-source=rnd -r -i 1-3 | head -5
shuf --random-source=rnd -r lines | head -5
shuf --random-source=rnd -r -n 3 empty
shuf --random-source=rnd -r -n 0 empty
cat lines | shuf --random-source=rnd -r -n 4

# --- -o ---
shuf --random-source=rnd -o out lines; cat out
shuf --random-source=rnd -o inplace inplace; cat inplace
shuf --random-source=rnd -o out -o out lines; cat out
shuf --random-source=rnd -o dir/ lines
shuf --random-source=rnd -o dir lines

# --- the random source ---
shuf --random-source=short thousand
shuf --random-source=short -n 1 -i 1-10
shuf --random-source=nonexistent lines
shuf --random-source=rnd nonexistent
shuf --random-source=rnd --random-source=rnd lines
shuf --random-source=rnd --random-source=short lines

# --- without a random source: only what the draw does not decide ---
shuf lines | sort
shuf -i 1-100 | sort -n | md5sum
shuf -e c b a | sort
shuf -n 3 lines | wc -l
shuf -r -n 7 -e x | uniq -c

# --- errors ---
shuf -i 6-4
shuf -i x
shuf -i 1-
shuf -i 1-2x
shuf -i -1-2
shuf -i 0-18446744073709551615
shuf -i 99999999999999999999999-5
shuf -i 1-99999999999999999999999
shuf -n x lines
shuf -n -1 lines
shuf -e -i 1-2 a
shuf a b
shuf -i 1-2 a
shuf -i 1-2 -i 3-4
shuf -Z
shuf --bogus
shuf -n
shuf --head
shuf nonexistent
shuf dir
shuf --random-source=rnd lines >/dev/full
shuf --random-source=rnd -i 1-100000 >/dev/full

# --- deliberately different ---
!--help text is ours|shuf --help
!--version text is ours|shuf --version
CASES

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

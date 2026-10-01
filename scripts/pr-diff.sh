#!/usr/bin/env bash
# pr-diff.sh — compare our `pr` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# `pr`'s output is a layout, so every case compares the bytes: standard
# output and standard error through one file (the order the two are
# interleaved in is part of the answer -- upstream flushes its output before
# every diagnostic), then the exit status.
#
# The header carries a date: the file's modification time, or the time now
# for standard input. The fixtures are given fixed times (nanoseconds
# included, for `-D %N`), and every standard-input case passes a `-D` whose
# answer cannot change while the two sides run.
#
# The fixtures are chosen to reach the interactions upstream's globals have:
# a file of 200 lines for page breaks and `+FIRST:LAST`, form feeds at the
# start of a page, doubled, and straight after a full page (the
# "coincidence" upstream drops), tabs, control and high bytes, backspaces,
# lines longer than every width used, files of different lengths for `-m`, a
# file without a final newline, an empty one, a directory, and a name that is
# not ASCII.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
#
# Under `LC_ALL=C`, GNU centres a header by counting bytes; the string layer
# here is UTF-8 in every locale (design-decisions.md §356), so a name that is
# not ASCII centres differently there, and only there.
set -u

DIFF_PROG='pr'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

export TZ=America/New_York
for v in $(env | sed -n 's/^\(LC_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
unset LANG LANGUAGE POSIXLY_CORRECT
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

GNU_PATH="$bindir/gnu:$PATH"
OURS_PATH="$bindir/ours:$PATH"
out_a="$DIFF_TMP/out-gnu"
out_b="$DIFF_TMP/out-ours"

mkdir -p "$DIFF_TMP/tree" || exit 1
cd "$DIFF_TMP/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---
printf 'line %s\n' 1 2 3 4 5 6 7 8 9 10 > f1
i=1
while [ "$i" -le 200 ]; do
    case $((i % 7)) in
        0) printf 'L%03d a somewhat longer line of the long file, to be cut by narrow columns\n' "$i" ;;
        3) printf 'L%03d\ttabbed\tfields\n' "$i" ;;
        *) printf 'L%03d text\n' "$i" ;;
    esac
    i=$((i + 1))
done > long
: > empty
printf 'x\ny' > noeol
{
    printf '%s\n' "$(printf 'w%.0s' $(seq 100))"
    printf 'short\n'
    printf 'tab\there then a long tail %s\n' "$(printf 'z%.0s' $(seq 80))"
    printf '%s\tend\n' "$(printf 'y%.0s' $(seq 70))"
} > wide
printf 'a\tb\tc\n\tlead\n12345678\tx\n1234567\ty\n:colon:sep:\n \t mixed\n' > 'tabs'
printf 'a\001b\007c\bd\re\177f\351g\377h\n\tx\013y\n\033[1mbold\033[0m\n' > ctrl
printf 'ab\b\bcd\n\b\bstart\nx\by\bz\n' > bs
printf 'one\ntwo\n\fthree\nfour\n\f\ffive\nsix\fseven\n\f\n' > ff
{
    i=1
    while [ "$i" -le 56 ]; do printf 'full %d\n' "$i"; i=$((i + 1)); done
    printf '\fafter the full page\nmore\n'
    i=1
    while [ "$i" -le 56 ]; do printf 'second %d\n' "$i"; i=$((i + 1)); done
    printf '\f\nafter a form feed and a newline\n'
} > ffpage
printf 'g1-%s\n' 1 2 3 > g1
printf 'g2-%s\n' 1 2 3 4 5 6 7 > g2
printf 'g3-%s\n' 1 2 3 4 5 6 7 8 9 10 11 12 > g3
printf 'first\nsecond\n' > 'ünï'
mkdir dir
touch -d '2024-03-05 06:07:08.123456789' f1 long empty noeol wide tabs ctrl bs ff ffpage g1 g2 g3 'ünï' dir

pr() { PATH=$PR_PATH command pr "$@"; }

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

    # Captured through files, not `$(...)`, which would drop the NUL bytes
    # `-v` and `-c` cases are measuring.
    ( PR_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( PR_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )

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
# --- one file, the default page ---
pr f1
pr long
pr empty
pr noeol
pr f1 long
pr wide
pr tabs
pr ctrl
pr bs
pr 'ünï'
pr -h 'A custom header' f1
pr -h '' f1
pr -h 'ÄÖÜ wide 漢字' f1
pr -D '%s %N' f1
pr -D '' f1
pr -D '%Y' -h x f1
pr -D '%A %B %d %j %p %Z %z' f1

# --- page ranges ---
pr +2 long
pr +2:3 long
pr +3:3 long
pr +4 long
pr --pages=2 long
pr --pages=2:2 long
pr +9 long
pr +2 f1
pr +0 long
pr +5x
pr +x long
pr + long
pr +3: long
pr +3:5x long
pr +99999999999999999999999 long
pr --pages=x long
pr --pages=3:2 long
pr --pages=0 long
pr -- +2 long
pr --pages=2 +3 long
pr +3 --pages=2 long
pr +2 +3
pr ++2 long
pr +2 ffpage
pr +2 -2 long
pr +2 -a -3 long
pr +2 -m g3 long

# --- page length and pagination ---
pr -l 10 long
pr -l 11 long
pr -l 1 f1
pr -l 12 long
pr -l 20 -d long
pr -d f1
pr -d -t long
pr -l 15 -f long
pr -f f1
pr -F long
pr -t f1
pr -T f1
pr -t long
pr -T long
pr ff
pr -t ff
pr -T ff
pr -l 12 ff
pr ffpage
pr -t ffpage
pr -l 61 ffpage
pr -2 ff
pr -2 -t ff
pr -2 ffpage
pr -a -2 ff
pr -m ff f1
pr -m -t ff g2
pr -m -T ff g2
pr -m ff ffpage

# --- columns down, across, and their separators ---
pr -2 long
pr -3 long
pr -5 long
pr -2 f1
pr -3 f1
pr -2 -t f1
pr -a -3 long
pr -a -2 f1
pr -a -3 -t f1
pr --columns=4 long
pr -2 wide
pr -2 tabs
pr -3 ctrl
pr -3 -w 40 long
pr -2 -W 30 wide
pr -2 -J long
pr -2 -J -S: long
pr -2 -s long
pr -2 -s: long
pr -2 -S long
pr -2 -S' | ' long
pr -2 -S'  ' long
pr -2 -w 50 -s long
pr -2 -W 50 -s long
pr -2 -w 50 -s: long
pr -2 -o 4 long
pr -2 -n long
pr -3 -n -a long
pr -2 -n: long
pr -2 -e long
pr -2 -i long
pr -2 -d long
pr -12 f1
pr -40 f1
pr -1 -2 f1
pr -2 -3 f1
pr -3 --columns=2 f1

# --- files in parallel ---
pr -m g1 g2 g3
pr -m -t g1 g2 g3
pr -m -n g1 g2
pr -m -n -t g3 g1
pr -m -J g1 g2
pr -m -S: g1 g2
pr -m -s g1 g2
pr -m -s: g1 g2
pr -m -w 40 g1 g2
pr -m -W 40 g1 g2
pr -m -w 40 -s g1 g2
pr -m g1 nosuch g2
pr -m nosuch nosuch2
pr -m -r g1 nosuch g2
pr -m g1
pr -m wide tabs
pr -m -o 3 g1 g2
pr -m -h 'M' g1 g2
pr -m g3 g1 g3 g1
pr -m -e tabs g1
pr -m -a g1

# --- line numbers ---
pr -n f1
pr -n long
pr -n3 f1
pr -n:3 f1
pr -nx f1
pr -n -N 100 long
pr -N 5 -n f1
pr -N -3 -n f1
pr -n +2 long
pr -n -N 7 +2 long
pr -n1 f1
pr -n99 -t f1
pr --number-lines=.2 f1
pr -n -o 3 f1
pr -n -i f1
pr -n -d -t f1
pr -n -2 -t long
pr -n -N 999999 -t f1

# --- tabs, non-printables, backspaces, truncation ---
pr -e tabs
pr -e:3 tabs
pr -e4 tabs
pr -i tabs
pr -i:4 tabs
pr -ix2 f1
pr -e -i tabs
pr -e -i -t tabs
pr -c ctrl
pr -v ctrl
pr -c -v ctrl
pr -c -2 ctrl
pr -t bs
pr -2 -t bs
pr -W 10 wide
pr -W 10 -t tabs
pr -J -W 10 wide
pr -w 10 wide
pr -W 12 -n -t long

# --- margins ---
pr -o 5 f1
pr -o 5 -t f1
pr -o 0 f1
pr -o 9 -i f1
pr -o 3 -2 -t long

# --- standard input ---
pr -D '%Y' < f1
pr -D '%Y' - < f1
pr -D '%Y' - - < f1
pr -D '%Y' f1 - < long
pr -t < ff
pr -m -D x - g1 < g2
pr -D x -2 -t < long
pr -D x <&-
pr -D x - <&-

# --- errors ---
pr nosuch
pr -r nosuch
pr f1 nosuch f1
pr dir
pr f1 dir f1
pr -l 0 f1
pr -l x f1
pr -l -5 f1
pr -l 99999999999 f1
pr -l 2147483647 -2 f1
pr -w 0 f1
pr -W x f1
pr -o -1 f1
pr -N x f1
pr -N 99999999999 f1
pr --columns=0 f1
pr --columns=x f1
pr -0 f1
pr -n:0 f1
pr -nx0 f1
pr -n:99999999999 f1
pr -n:5x f1
pr --number-lines= f1
pr --expand-tabs= f1
pr -e:x f1
pr -m -2 f1 f1
pr -m -a f1 f1
pr -Z f1
pr --bogus f1
pr --pag=2 long
pr --col=2 long
pr --number f1
pr -n -w 5 -m f1 f1
pr -W 3 -5 f1
pr -D
pr -h
pr --pages
pr f1 >/dev/full
pr -t f1 >/dev/full

# --- the environment ---
POSIXLY_CORRECT=1 LC_ALL=C pr f1
POSIXLY_CORRECT=1 LC_ALL=C.UTF-8 pr f1
POSIXLY_CORRECT=1 LC_ALL= LANG=C pr f1
pr f1 -n
POSIXLY_CORRECT=1 pr f1 -n
LC_ALL=C pr -D '%b %a' f1
TZ=UTC pr f1
TZ=Asia/Kolkata pr -D '%H:%M %Z' f1

# --- deliberately different ---
!--help text is ours|pr --help
!--version text is ours|pr --version
!a header is centred in columns, not bytes, in every locale|LC_ALL=C pr 'ünï'
CASES

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

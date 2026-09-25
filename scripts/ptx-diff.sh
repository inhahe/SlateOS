#!/usr/bin/env bash
# ptx-diff.sh — compare our `ptx` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# Each case is one shell command line; standard output and standard error go
# to one file, compared byte for byte with the exit status after them.
#
# `ptx` lays every word of its input out on a line of its own, the word at
# the centre and its context either side, cut to fit and flagged where it was
# cut -- so the fixtures are text chosen to reach each field: sentences ending
# with and without closing marks, keywords near the start and the end of long
# lines (where the "head" and "tail" wrap-around fields are used), lines
# longer than every width tried, references at the start of lines for `-r`,
# ignore, only and break files, words that are not ASCII, a file without a
# final newline, an empty one, and one that trips upstream's "match of length
# zero" error.
#
# The regular expressions of `-S` and `-W` are glibc's Emacs syntax, so a
# number of cases are there to tell that syntax from ERE: `\(…\|…\)` grouping,
# `[[:alpha:]]` meaning its six members rather than a class, `\w`.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
#
# A `-W` pattern that can match the empty string: upstream steps over a word
# by `re_match`'s length, which is then zero, and never finishes; ours steps
# one byte and does (see `ptx.rs`). The case runs under a timeout.
set -u

DIFF_PROG='ptx'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

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
cat > t1 <<'EOF'
The quick brown fox jumps over the lazy dog.  It barked!
"Stop," said the fox.	He ran (quickly) away, into the woods.
A line without an end
Numbers 42 and words_with_underscores, e.g. etc. and more?  Yes.
EOF
printf 'Second file here.  More text\nand a last line\n' > t2
{
    printf 'This line is a very long line indeed, much longer than the output width, so that the fields have to be cut and flagged at both ends of the keyword in the middle of it all.\n'
    printf 'short\n'
    printf 'Another long one: alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega.\n'
} > long
printf 'ref1 some words here\nref2 more words there\n  ref3 indented reference line\nref4\n' > refs
printf 'the\nand\n\nof\n' > ignore
printf 'fox\ndog\nwords\n' > only
printf ' ,.!?\n' > breaks
printf 'café naïve résumé\nÜber straße\n' > nonascii
printf 'no newline at the end' > noeol
: > empty
printf 'A.\n.\nB\n' > zerolen
printf 'Word\tafter\ttabs and\fform feeds\vand vtabs\r\n' > space
# `%%`: a bare `% &` is a conversion to printf, which stops the fixture there.
printf '$money%% & #hash _under {brace} back\\slash "quoted"\n' > specials
mkdir dir

ptx() { PATH=$PTX_PATH command ptx "$@"; }

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

    ( PTX_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( PTX_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )

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
# --- the default index ---
ptx t1
ptx t1 t2
ptx long
ptx refs
ptx nonascii
ptx noeol
ptx empty
ptx space
ptx specials
ptx t1 empty t2

# --- widths, gaps, truncation marks ---
ptx -w 40 t1
ptx -w 100 t1
ptx -w 20 long
ptx -w 10 long
ptx -w 1 t1
ptx -w 0x30 t1
ptx -w 010 t1
ptx -g 1 t1
ptx -g 10 t1
ptx -g 40 t1
ptx -F '...' long
ptx -F '' long
ptx -F '\t' long
ptx -F '\x3e\x3c' long
ptx -F 'a\0b' long
ptx -F 'x\cy' long
ptx -w 30 -g 1 -F '<>' long

# --- references ---
ptx -A t1
ptx -A t1 t2
ptx -A -R t1
ptx -A -w 30 t1 t2
ptx -R t1
ptx -r refs
ptx -r -R refs
ptx -r -w 30 refs
ptx -A -r refs
ptx -A - < t1
ptx -A t1 - < t2

# --- sentence and word patterns ---
ptx -S '' t1
ptx -S '\n' t1
ptx -S '[.!?]' t1
ptx -S '\.' t1
ptx -S '\(,\|\.\)' t1
ptx -S 'e' t1
ptx -W '[a-z]+' t1
ptx -W '[[:alpha:]]+' t1
ptx -W '\w+' t1
ptx -W '[a-zA-Z_]+' t1
ptx -W '[0-9]+' t1
ptx -W '.' t2
ptx -W '\<[a-z]' t1
ptx -W 'o+' t1
ptx -W '' t1
ptx -f t1
ptx -f -W '[A-Z]+' t1
ptx -W '\(fox\|dog\)' t1
ptx -W '(fox|dog)' t1
ptx zerolen
ptx -S '\n' zerolen
ptx -G zerolen

# --- word lists and break characters ---
ptx -i ignore t1
ptx -o only t1
ptx -i ignore -o only t1
ptx -i empty t1
ptx -o empty t1
ptx -f -i ignore t1
ptx -b breaks t1
ptx -G -b breaks t1
ptx -b empty t1

# --- output formats ---
ptx -O t1
ptx -T t1
ptx --format=roff t1
ptx --format=tex -A t1
ptx --format=r t1
ptx --form=te t1
ptx -O -M idx t1
ptx -T -M ix long
ptx -O specials
ptx -T specials
ptx -O -A -R t1
ptx -T -r refs

# --- traditional mode ---
ptx -G t1
ptx -G -A t1
ptx -G -O t1
ptx -G -T t1
ptx -G -w 30 long
rm -f out1; ptx -G t1 out1; cat out1
rm -f out2; ptx -G t1 out2 extra; ls out2
rm -f out3; ptx -G nosuch out3; ls out3
ptx -G t1 /nonexistent/dir/out
ptx -G - < t1
ptx -G t1 /dev/full

# --- standard input ---
ptx < t1
ptx - < t1
ptx '' < t1
ptx - - < t1
ptx t1 - < t2
ptx <&-
ptx - <&-

# --- errors ---
ptx nosuch
ptx t1 nosuch
ptx dir
ptx -i nosuch t1
ptx -o nosuch t1
ptx -b nosuch t1
ptx -w 0 t1
ptx -w x t1
ptx -w -3 t1
ptx -w 99999999999999999999 t1
ptx -g 0 t1
ptx -g 1x t1
ptx --format=x t1
ptx --format= t1
ptx -W '\(' t1
ptx -W 'a\)' t1
ptx -S '[' t1
ptx -S '[a' t1
ptx -W '\(a\)\2' t1
ptx -W '[[.ab.]]' t1
ptx -W '[z-a]' t1
ptx -W 'x\' t1
ptx -t t1
ptx -Z t1
ptx --bogus t1
ptx --ref t1
ptx t1 >/dev/full

# --- locales ---
LC_ALL=C ptx nonascii
LC_ALL=C ptx -f t1

# --- deliberately different ---
!--help text is ours|ptx --help
!--version text is ours|ptx --version
!GNU never ends on an empty keyword match|PATH=$PTX_PATH timeout 10 ptx -W '[a-z]*' t1
CASES

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

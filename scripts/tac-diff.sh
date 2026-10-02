#!/usr/bin/env bash
# tac-diff.sh — compare our `tac` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# Each case is one shell command line; standard output and standard error go
# to one file, compared byte for byte with the exit status after them.
#
# `tac` reads a file backwards a buffer at a time -- 8 KiB to begin with,
# doubling when a record outgrows it, and the size carried from one file to
# the next -- and with `-r` that reading is observable: the pattern is
# searched within the part of the buffer not yet printed, so `^` and `$` hold
# at the buffer's edges as well as at newlines. So the fixtures are chosen to
# cross those edges: files of exactly one and two reads, records longer than a
# read, a file of five thousand lines, and patterns anchored at both ends run
# over them. Beside those, the separators: multi-byte, overlapping, NUL (what
# `-s ''` means without `-r`), and `-b` with each. Standard input comes as a
# file (seekable, read in place) and as a pipe (copied to a temporary file
# first), twice in one command line, and with `TMPDIR` pointing nowhere.
#
# The patterns are glibc's Emacs syntax, which `re_compile_pattern` reads by
# default: `\(…\|…\)` groups, `+` and `?` operators, `[[:alpha:]]` as a
# bracket of six members, and back-references.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='tac'
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

printf 'one\ntwo\nthree\n' > lines
printf 'one\ntwo\nthree' > noeol
printf 'only\n' > single
printf '\n\n\n' > blanks
: > empty
printf 'a;b;;c;' > semis
printf 'x;;y;;;z' > semis2
printf 'aaaaa' > a5
printf 'xabyababz' > ab
printf 'r1\0r2\0\0r3' > nuls
printf 'dos\r\nlines\r\nhere\r\n' > crlf
printf 'café\nnaïve\nrésumé\n' > utf8
printf 'line\xffwith\xfe\nbad bytes\n' > badutf8
seq 1 5000 > big
# Exactly one read, exactly two, and one byte over.
head -c 8192 big > r8192
head -c 16384 big > r16384
head -c 16385 big > r16385
# Records longer than a read: the buffer has to grow, and grow again.
{ head -c 20000 /dev/zero | tr '\0' 'a'; printf '\n'; head -c 40000 /dev/zero | tr '\0' 'b'; printf '\nend\n'; } > longrec
# Many short records with a separator that recurs inside them.
awk 'BEGIN { for (i = 0; i < 3000; i++) printf "rec%d;;sep;", i }' > manyseps
# Binary noise, fixed so a failure can be rerun.
awk 'BEGIN { srand(7); for (i = 0; i < 40000; i++) printf "%c", int(rand() * 256) }' > noise
mkdir dir
printf 'locked\n' > unreadable
chmod 000 unreadable

tac() { PATH=$TAC_PATH command tac "$@"; }

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
    ( TAC_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( TAC_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )
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
# --- plain records ---
tac lines
tac noeol
tac single
tac blanks
tac empty
tac lines noeol single
tac crlf
tac utf8
tac badutf8
tac big | md5sum
tac r8192 | md5sum
tac r16384 | md5sum
tac r16385 | md5sum
tac longrec | md5sum
tac noise | md5sum
tac big lines big | md5sum

# --- -b ---
tac -b lines
tac -b noeol
tac --before blanks
tac -b big | md5sum
tac -b longrec | md5sum

# --- -s ---
tac -s ';' semis
tac -s ';' semis2
tac -b -s ';' semis
tac -s ';;' semis2
tac -b -s ';;' semis2
tac -s aa a5
tac -b -s aa a5
tac -s ab ab
tac -s '' nuls
tac -b -s '' nuls
tac --separator=';' semis
tac -s ';;sep;' manyseps | md5sum
tac -b -s ';;sep;' manyseps | md5sum
tac -s "$(printf '\n\n')" blanks
tac -s xyz lines
tac -s "$(head -c 9000 /dev/zero | tr '\0' x)" lines

# --- -r: Emacs syntax ---
tac -r -s '[0-9]' lines
tac -r -s 'o\|e' lines
tac -r -s 'o+' lines
tac -r -s 'e?n' lines
tac -r -s '\(on\|re\)' lines
tac -r -s '[[:alpha:]]' lines
tac -r -s '.' noeol
tac -r -s '.' utf8
tac -r -s '.' badutf8
tac -r -s ';+' semis2
tac -b -r -s ';+' semis2
tac -r -s '\(a\)\1' a5
tac -r -s 'a\{2\}' a5
tac -r -s "$(printf '\n')" lines
tac -r -s '\w+' utf8
tac -r -s '\bt' lines
tac -r -s '[0-9]+' big | md5sum
tac -b -r -s '[0-9]+' big | md5sum
tac -r -s '0$' big | md5sum
tac -r -s '^9' big | md5sum
tac -r -s '^' lines
tac -r -s '$' lines
tac -r -s '\`.' lines
tac -r -s ".\\'" lines
tac -r -s 'b+' longrec | md5sum
tac -r -s '[a-z]*' lines
tac -r -s ';;sep;' manyseps | md5sum
tac -r -s '[^;]+' manyseps | md5sum
tac -r -s '.' noise | md5sum

# --- -r: refused ---
tac -r -s '' lines
tac -r -s '\(' lines
tac -r -s '\)' lines
tac -r -s '[' lines
tac -r -s 'a\{1' lines
tac -r -s '\1' lines

# --- standard input ---
tac < lines
tac - < lines
tac - - < lines
tac lines - < noeol
cat lines | tac
cat lines | tac - -
cat big | tac | md5sum
cat longrec | tac -b | md5sum
cat semis | tac -s ';'
cat lines | TMPDIR=/nonexistent tac
cat lines | TMPDIR= tac
tac < /dev/null

# --- locales ---
LC_ALL=C tac -r -s '.' utf8
LC_ALL=C tac -r -s '[é]' utf8
LANG=C.UTF-8 LC_ALL= tac -r -s '.' utf8

# --- errors ---
tac nonexistent
tac lines nonexistent noeol
tac dir
tac unreadable
tac -Z
tac --bogus lines
tac -s
tac --separator
tac --bef lines
tac lines >/dev/full
tac -b big >/dev/full

# --- deliberately different ---
!--help text is ours|tac --help
!--version text is ours|tac --version
CASES

chmod 600 unreadable

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

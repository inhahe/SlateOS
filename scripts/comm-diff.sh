#!/usr/bin/env bash
# Differential test: our comm against GNU comm.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The reference has to be glibc's: the
# host's `comm` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll` rather
# than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`), so a harness pointed
# at it certifies sentences no GNU/Linux system prints (`known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`). This file already
# avoided that by reaching for `wsl -e env LC_ALL=$loc comm`, at the cost of a
# WSL process per case and a probe to check that `wsl`'s inherited Windows cwd
# landed on the same bytes under `/mnt/...`.
#
# The subject moving with it is the part that changed an answer — see the two
# directory-operand cases at the foot of this file, which were expected
# differences only because a Windows `File::open` refuses a directory outright.
#
# The locale stays a per-case argument to `run_side` rather than being pinned
# once at the top, for the reason the next section gives: here it is not a
# formatting knob but part of what is being compared.
#
# Run `OURS=/usr/bin/comm ./scripts/comm-diff.sh` to confirm the harness still
# discriminates: every expected difference should turn into an XPASS.
#
# ## Why the comparison cases run under `LC_ALL=C` and the diagnostics do not
#
# Its siblings run wholly under `C.UTF-8`. `comm` cannot, because for the cases
# that actually compare something the locale decides *what the program
# computes*, not merely how it words a complaint.
#
#     hard_LC_COLLATE = hard_locale (LC_COLLATE);
#     ...
#     order = hard_LC_COLLATE ? xmemcoll (...) : memcmp2 (...);
#
# `hard_locale` is false for exactly `C` and `POSIX`. So under any other locale
# GNU `comm` pairs lines by `strcoll` — a locale collation in which case can be
# secondary, punctuation can be ignored at the first level, and two different
# byte strings can compare equal. Ours compares bytes, always. Under `C` that
# *is* GNU's rule and the two agree by construction; under `C.UTF-8` they agree
# only for inputs where codepoint order happens to coincide with byte order, and
# a harness that ran there would be certifying a coincidence.
#
# The divergence itself is real and is not this harness's to hide: it is written
# up in `comm.rs`'s module documentation and filed against the same collation
# entry in `known-issues.md` as the `oils` funmap listing. The last section here
# re-runs a few comparison cases under `C.UTF-8` to record how far the agreement
# actually extends.
#
# None of that reaches the diagnostics — a message about a missing operand
# collates nothing — and since §351 they must be referenced under `C.UTF-8`,
# because our `quote()` now prints U+2018/U+2019 in every locale and GNU prints
# those only under a UTF-8 one. So they run through `run_diag`, not `run_case`.
#
# ## Why `od -An -c`
#
# `comm`'s output *is* its separators — a tab per suppressed-or-not column by
# default, a NUL under `--output-delimiter=`, and whatever `--output-delimiter`
# was handed otherwise, up to and including bytes that are not text. A
# comparison that trimmed or collapsed whitespace would agree with almost every
# wrong implementation: the difference between `comm -1` and `comm -2` output is
# entirely a question of how many leading tabs each line carries.
#
# ## Cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's;
# and `comm - -`, where GNU's exit status is the visible edge of undefined
# behaviour. `comm` closes both files when it is done, and with `-` twice both
# handles are the one `stdin`, so the second `fclose` is called on a stream that
# has already been closed — C says nothing about what happens next. On glibc it
# sets the error indicator, and `close_stdout` then reports
# `comm: -: Bad file descriptor` and exits 1. The merged output is identical on
# both sides; reproducing the diagnostic would mean reproducing a double free.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `comm` so `argv[0]` matches. See `scripts/diff-wsl.sh`.  `timeout` is
# named because every invocation below is bounded with it; see `run_side`.
DIFF_PROG=comm
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# Every invocation is bounded, on both sides. `comm`'s merge advances only the
# file that lost the comparison, so an implementation that returned a stale line
# instead of advancing would emit that line forever. That is the specific
# failure this timeout exists for, and it is why the *reference* is wrapped too:
# a harness that only bounded our side would hang on the day the reference was
# the buggy one.
#
# One invocation of one side. `$1` is `ours` or `gnu`; each is reached through
# a symlink named `comm` in a directory that is the whole of `PATH` for that one
# invocation, so `argv[0]` is the bare word on both sides. The locale stays a
# per-case argument because half the cases here are *about* the locale; it is
# spliced into the same `env` that narrows `PATH`.
# `diff_run` keeps bash's own announcement of a child that died of a signal
# out of the stderr the caller captures; `diff-wsl.sh` says why.
run_side() {
  local side=$1 loc=$2; shift 2
  diff_run timeout -k 2 30 env "LC_ALL=$loc" PATH="$bindir/$side" comm "$@"
}

# --- fixtures ----------------------------------------------------------------
# The canonical pair: one line only in the first, one only in the second, one in
# both, so a single run exercises all three columns.
printf 'a\nb\nd\n'         > a.txt
printf 'b\nc\nd\n'         > b.txt
# Disjoint, so one column is always empty and the other never pairs.
printf 'x\ny\n'            > x.txt
# Identical to a.txt, so every line pairs and the outer columns stay empty.
printf 'a\nb\nd\n'         > same.txt
# Empty: whichever side it is on, the other must drain into its own column.
printf ''                  > empty.txt
# No final newline, which `readlinebuffer_delim` supplies — so the last line
# pairs with a terminated line elsewhere rather than sorting after it.
printf 'a\nz'              > unterm.txt
# A single unterminated line, so the first record read is also the last.
printf 'solo'              > solo.txt
# Out of order, for the order check. `c` before `a` is a descent that only
# matters once something fails to pair.
printf 'c\na\nb\n'         > dis.txt
printf 'a\nb\nc\n'         > sorted.txt
# Duplicates on both sides in unequal numbers: comm pairs them one for one and
# the surplus falls out into the outer columns.
printf 'a\na\nb\nb\nb\n'   > dup1.txt
printf 'a\nb\nb\n'         > dup2.txt
# Blank lines, which are lines: they sort before everything and must pair.
printf '\n\n\nq\n'         > blanks.txt
printf '\nq\n'            > blank1.txt
# One line a strict prefix of another, which is the length tiebreak that a
# comparison written as a bare memcmp over the shorter length would get wrong.
printf 'a\nab\nabc\n'      > pre1.txt
printf 'ab\nabcd\n'        > pre2.txt
# Bytes that are not text at all, in the data. Sorted bytewise, which under `C`
# is what both sides use.
printf '\x01\n\x80\n\xff\n' > bad1.txt
printf '\x80\n\xfe\n'      > bad2.txt
# Lines holding the characters the --output-delimiter cases use, so a separator
# is never confusable with content by accident alone.
printf 'a:b\nc:d\n'        > colons.txt
printf 'a:b\nx:y\n'        > colons2.txt
# NUL-separated, for `-z`; note both also hold newlines, which under `-z` are
# ordinary content and must survive untouched.
printf 'a\0b\0d\0'         > nul1.txt
printf 'b\0c\0d\0'         > nul2.txt
printf 'p\nq\0r\0'         > nulnl.txt
# Long enough to cross the reader's buffer boundary, and overlapping only in
# part so the merge keeps switching sides across that boundary.
{ for i in $(seq 1000 5999); do printf 'line%04d\n' "$i"; done; } > long1.txt
{ for i in $(seq 3000 7999); do printf 'line%04d\n' "$i"; done; } > long2.txt
mkdir subdir

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(comm | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$loc" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_side ours "$loc" "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_side gnu  "$loc" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of the
  # getopt module is that the sentences match, so a harness that only asked "did
  # it complain?" would pass on every wording this exists to fix. It also
  # matters for the order check, which prints *at most one* warning per file —
  # a implementation that warned on every descent would still "complain".
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err"
}

report() {
  local label="$1"; shift
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case()  { compare - C "$@"; report "comm $*"; }
# The same case under a locale where GNU collates rather than compares bytes.
# Not a synonym for `run_case`: it is the measurement behind the claim that the
# divergence is confined to inputs whose collation order differs from their byte
# order. See the header.
run_utf8()  { compare - C.UTF-8 "$@"; report "comm $* [C.UTF-8]"; }
# The diagnostics. They reach `C.UTF-8` by a different road than `run_utf8`
# does, so they keep a name of their own even though the mechanism is the same:
# they pair nothing and collate nothing, so the header's reason for `C` never
# reaches them, and what does reach them is §351, which made our `quote()` print
# U+2018/U+2019 in every locale. GNU prints those under a UTF-8 locale and ASCII
# under `C`, so `C` is now the setting in which the reference would be wrong.
# Spelled as a call rather than a copy so the two cannot silently drift apart.
run_diag()  { run_utf8 "$@"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" C "$@"
  report "printf '$input' | comm $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  local why="$1"; shift
  compare - C "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS comm %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL comm %s  (%s)\n' "$*" "$why"
  fi
  return 0
}
xfail_stdin() {
  local why="$1" input="$2"; shift 2
  compare "$input" C "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS comm %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL comm %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the three columns --------------------------------------------------------
# The shipped parser read `--`, `-z`, `--total` and every other long option as
# filenames, so a good many of these fail against it before any of the merge
# rules are involved.
run_case a.txt b.txt
run_case b.txt a.txt
run_case a.txt same.txt
run_case a.txt a.txt
run_case a.txt x.txt
run_case x.txt a.txt
run_case dup1.txt dup2.txt
run_case dup2.txt dup1.txt
run_case blanks.txt blank1.txt
run_case blank1.txt blanks.txt
run_case pre1.txt pre2.txt
run_case pre2.txt pre1.txt
run_case bad1.txt bad2.txt
run_case bad2.txt bad1.txt
run_case long1.txt long2.txt
run_case long2.txt long1.txt
run_case long1.txt long1.txt

# --- one side runs out --------------------------------------------------------
run_case a.txt empty.txt
run_case empty.txt b.txt
run_case empty.txt empty.txt
run_case a.txt solo.txt
run_case solo.txt a.txt
run_case solo.txt solo.txt

# --- the delimiter that was not there -----------------------------------------
# A file whose last line has no delimiter gains one, so it pairs with the same
# text elsewhere instead of sorting after it.
run_case unterm.txt sorted.txt
run_case sorted.txt unterm.txt
run_case unterm.txt unterm.txt
run_case unterm.txt empty.txt
run_case empty.txt unterm.txt

# --- column suppression -------------------------------------------------------
# Each suppressed column takes its separator with it, so the leading tabs on
# every *remaining* line change too — which is the whole content of these rows.
for opts in -1 -2 -3 -12 -13 -23 -123 -21 -31 -321; do
  run_case "$opts" a.txt b.txt
done
run_case -1 -2 a.txt b.txt
run_case -1 -1 a.txt b.txt
run_case -3 -1 -2 a.txt b.txt
run_case -12 a.txt empty.txt
run_case -123 a.txt b.txt
run_case -123 empty.txt empty.txt
run_case -12 dup1.txt dup2.txt
run_case -3 long1.txt long2.txt

# --- --total ------------------------------------------------------------------
# The counts are of *lines*, not of lines printed: a suppressed column still
# counts. The row itself is separated by the same delimiter as the columns.
run_case --total a.txt b.txt
run_case --total -12 a.txt b.txt
run_case --total -123 a.txt b.txt
run_case --total empty.txt empty.txt
run_case --total a.txt empty.txt
run_case --total empty.txt b.txt
run_case --total a.txt same.txt
run_case --total dup1.txt dup2.txt
run_case --total long1.txt long2.txt
run_case --tot a.txt b.txt
run_case --total --total a.txt b.txt

# --- --output-delimiter -------------------------------------------------------
# One separator per column boundary, so a multi-byte argument multiplies.
for d in : :: ',;' 'XY' '  ' '.'; do
  run_case --output-delimiter="$d" a.txt b.txt
  run_case --output-delimiter="$d" --total a.txt b.txt
  run_case --output-delimiter="$d" -1 a.txt b.txt
done
run_case --output-delimiter=: colons.txt colons2.txt
run_case --output-delimiter , a.txt b.txt
run_case --out : a.txt b.txt
run_case --output-del=: a.txt b.txt
# The empty argument is a NUL, not nothing: `col_sep` still points at "" but
# `col_sep_len` is forced to 1, so one zero byte is written per boundary.
run_case --output-delimiter= a.txt b.txt
run_case --output-delimiter= --total a.txt b.txt
run_case --output-delimiter= -2 a.txt b.txt
run_case --output-delimiter '' a.txt b.txt
# Repeating it is allowed only with the identical argument — compared as typed,
# so two empties agree even though what they mean is not what they say.
run_case --output-delimiter=: --output-delimiter=: a.txt b.txt
run_case --output-delimiter= --output-delimiter= a.txt b.txt
run_case --output-delimiter=:: --output-delimiter=:: a.txt b.txt
# The refusal quotes nothing, so it is an ordinary case rather than an ASCII one.
run_case --output-delimiter=: --output-delimiter=';' a.txt b.txt
run_case --output-delimiter= --output-delimiter=: a.txt b.txt
run_case --output-delimiter=: --output-delimiter= a.txt b.txt
run_case --output-delimiter=: --output-delimiter=:: a.txt b.txt
# It is refused before the files are opened…
run_case --output-delimiter=: --output-delimiter=';' nosuch.txt nosuch2.txt
# …and after the whole command line is read, so a getopt error preempts it.
run_diag --output-delimiter=: --output-delimiter=';' -Q a.txt b.txt
# Bytes that are not text, as separators.
run_case --output-delimiter='é' a.txt b.txt
run_case --output-delimiter='中' --total a.txt b.txt

# --- -z, which changes what a line is -----------------------------------------
run_case -z nul1.txt nul2.txt
run_case -z nul2.txt nul1.txt
run_case -z nulnl.txt nul2.txt
run_case -z a.txt b.txt
run_case -z empty.txt nul1.txt
run_case -z nul1.txt empty.txt
run_case -z --total nul1.txt nul2.txt
run_case -z -12 nul1.txt nul2.txt
run_case --zero-terminated nul1.txt nul2.txt
run_case --zero nul1.txt nul2.txt
run_case -z --output-delimiter=: nul1.txt nul2.txt
run_case -z --output-delimiter= --total nul1.txt nul2.txt
run_case -1z nul1.txt nul2.txt
run_case -z1 nul1.txt nul2.txt
run_case -z unterm.txt sorted.txt

# --- the order check ----------------------------------------------------------
# Three states, and they are not "warn / warn / quiet". By default the check is
# armed only once some line has failed to pair, so a wholly disordered pair of
# *identical* files is silent — every line pairs, so nothing ever arms it.
run_case dis.txt sorted.txt
run_case sorted.txt dis.txt
run_case dis.txt dis.txt
run_case dis.txt a.txt
run_case a.txt dis.txt
run_case --check-order dis.txt sorted.txt
run_case --check-order sorted.txt dis.txt
run_case --check-order dis.txt dis.txt
run_case --check-order a.txt b.txt
run_case --check-order sorted.txt sorted.txt
run_case --nocheck-order dis.txt sorted.txt
run_case --nocheck-order dis.txt dis.txt
run_case --nocheck-order sorted.txt dis.txt
# The last one wins, in both directions.
run_case --check-order --nocheck-order dis.txt sorted.txt
run_case --nocheck-order --check-order dis.txt sorted.txt
run_case --check --nocheck dis.txt sorted.txt
run_case --noc dis.txt sorted.txt
run_case --check dis.txt sorted.txt
# At most one warning per file however many descents there are.
printf 'z\ny\nx\nw\n' > desc.txt
run_case desc.txt sorted.txt
run_case --check-order desc.txt desc.txt
run_case desc.txt desc.txt
# With a column suppressed the warning still goes to stderr and the status is
# still 1, so the two streams are checked independently.
run_case -12 dis.txt sorted.txt
run_case --check-order -123 dis.txt sorted.txt
run_case --total dis.txt sorted.txt

# --- standard input -----------------------------------------------------------
run_stdin 'a\nb\nd\n' - b.txt
run_stdin 'b\nc\nd\n' a.txt -
run_stdin 'a\nb\nd\n' -12 - b.txt
run_stdin '' - a.txt
run_stdin 'a\nz' - sorted.txt
run_stdin 'a\nb\nd\n' --total - b.txt
run_stdin 'c\na\nb\n' - sorted.txt

# --- operands are counted exactly ---------------------------------------------
# Fewer than two and more than two are different sentences, and the one-operand
# sentence names the operand — which after glibc's permutation is the last
# argument, whichever position it was typed in.
run_diag
run_diag a.txt
run_diag -z a.txt
run_diag --total a.txt
run_diag a.txt -z
run_diag nosuch.txt
run_diag -
run_diag a.txt b.txt empty.txt
run_diag a.txt b.txt empty.txt sorted.txt
run_diag -12 a.txt b.txt empty.txt
run_diag a.txt empty.txt b.txt

# --- operands that cannot be opened -------------------------------------------
# The first file is opened first, so a pair of bad ones names only the first.
run_diag nosuch.txt nosuch2.txt
run_diag a.txt nosuch.txt
run_diag nosuch.txt a.txt
run_diag --total nosuch.txt a.txt
run_diag -12 a.txt nosuch.txt

# --- getopt diagnostics -------------------------------------------------------
run_diag -Q a.txt b.txt
run_diag -1Q a.txt b.txt
run_diag -Q1 a.txt b.txt
run_diag -0 a.txt b.txt
run_diag -4 a.txt b.txt
run_diag -9 a.txt b.txt
run_diag --nope a.txt b.txt
run_diag --total=x a.txt b.txt
run_diag --check-order=x a.txt b.txt
run_diag --nocheck-order=x a.txt b.txt
run_diag --zero-terminated=x a.txt b.txt
run_diag --help=x a.txt b.txt
run_diag --version=x a.txt b.txt
run_diag --output-delimiter
run_diag --output-delimiter a.txt
run_diag --=x a.txt b.txt
run_diag -- -Q
# A getopt error beats a bad operand count, and both beat a missing file.
run_diag -Q
run_diag -Q a.txt
run_diag --nope

# --- abbreviations, which the shipped parser did not accept at all ------------
run_case --t a.txt b.txt
run_case --z nul1.txt nul2.txt
run_case --zero-t nul1.txt nul2.txt
run_case --o : a.txt b.txt
run_case --outp=: a.txt b.txt
run_diag --c dis.txt sorted.txt
run_diag --n dis.txt sorted.txt
run_diag --ch dis.txt sorted.txt

# --- options and operands interleave ------------------------------------------
run_case a.txt -1 b.txt
run_case a.txt b.txt -1
run_case -- a.txt b.txt
run_case -1 -- a.txt b.txt
run_case a.txt -- b.txt
run_case --total -- a.txt b.txt

# --- how far the byte comparison and GNU's collation agree --------------------
# Under `C.UTF-8` GNU switches to `strcoll`. For these inputs codepoint order
# and byte order coincide, so the two still agree; the row exists to catch the
# day that stops being true, and to bound the claim in the header. The
# non-text-byte fixtures are deliberately absent: an invalid UTF-8 sequence has
# no codepoint to collate, and what glibc does with it is not a rule we mean to
# reproduce.
run_utf8 a.txt b.txt
run_utf8 -12 a.txt b.txt
run_utf8 --total dup1.txt dup2.txt
run_utf8 pre1.txt pre2.txt
run_utf8 long1.txt long2.txt

# A directory operand in each position, which used to be an expected difference
# and is not one any more: opening a directory succeeds on POSIX and the *read*
# fails, so GNU says `subdir: Is a directory` with status 1 and so do we. They
# differed only while the subject was a Windows build, where `File::open`
# refuses a directory outright and the errno was the host's — see the same trap
# in `filekind.rs`. Moving both sides into WSL deleted them, as it did the
# identical case in `cut-diff.sh`, `fold-diff.sh`, `expand-diff.sh`,
# `unexpand-diff.sh`, `uniq-diff.sh`, `paste-diff.sh` and `tsort-diff.sh`.
run_case subdir a.txt
run_case a.txt subdir

# --- differ on purpose --------------------------------------------------------
# `comm - -` merges one stream against itself, which is well defined and which
# both sides get right; what differs is the ending. GNU closes both files, and
# both are `stdin`, so the second `fclose` operates on an already-closed stream.
# See the header.
xfail_stdin "GNU's second fclose(stdin) is undefined behaviour" 'a\nb\nc\n' - -

xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are here too, and as xfails rather than as ordinary
# cases: they reach the same two texts, so a `run_case` row for them would
# report the *body* difference we already accept and hide the thing they are
# actually for — that `--h` and `--v` resolve at all rather than being rejected
# as unknown. An XPASS on either would mean the resolution broke in the
# direction that makes them print something else entirely.
xfail_case 'an abbreviation of --help reaches our help text' --h a.txt b.txt
xfail_case 'an abbreviation of --version reaches our version text' --v a.txt b.txt

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1

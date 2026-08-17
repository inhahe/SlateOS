#!/usr/bin/env bash
# Differential test: our join against GNU join.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `join` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `comm-diff.sh`, `paste-diff.sh`, `fold-diff.sh`,
# `expand-diff.sh`, `head-diff.sh`, `wc-diff.sh`, `cut-diff.sh`, `uniq-diff.sh`
# and `nl-diff.sh`.
#
# Run `OURS=/usr/bin/join ./scripts/join-diff.sh` to confirm the harness still
# discriminates: it should report dozens of differences, not zero.
#
# ## Why the whole harness runs under `LC_ALL=C`, not just the diagnostics
#
# The same reason as `comm`: the locale decides *what the program computes*.
#
#     hard_LC_COLLATE = hard_locale (LC_COLLATE);
#     ...
#     diff = hard_LC_COLLATE ? xmemcoll (...) : memcmp (...);
#
# `hard_locale` is false for exactly `C` and `POSIX`, so under any other locale
# GNU `join` pairs keys by `strcoll` — a collation in which case can be
# secondary, punctuation can be ignored at the first level, and two different
# byte strings can compare equal, which for `join` means two different keys can
# *pair*. Ours compares bytes, always. Under `C` that is GNU's own rule and the
# two agree by construction. The last section re-runs a few cases under
# `C.UTF-8` to record how far the agreement extends; the divergence itself is
# written up in `join.rs`'s module documentation.
#
# Note that `-i` is not affected: `memcasecmp` is used whatever the locale, and
# it folds with `toupper`, so under `C` it folds ASCII and nothing else.
#
# ## Why `od -An -c`
#
# `join`'s output *is* its separators. The default output separator is a space
# even when the input was split on runs of blanks, `-t` changes both at once,
# `-e` fills a field that would otherwise be empty, and `-o auto` pads short
# lines with separators and nothing between them. A comparison that collapsed
# whitespace would agree with almost every wrong implementation, because for
# `join` almost every interesting difference *is* whitespace.
#
# ## Cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's,
# and a directory operand: on any POSIX host opening a directory succeeds and
# the *read* fails, so GNU reaches `read error`, while a Windows `File::open`
# refuses outright with the host's errno.
set -u

# Our join is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/join.exe"}
export LC_ALL=${LC_ALL:-C}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# Every invocation is bounded, on both sides. `join`'s merge advances only the
# file that lost the comparison, and its run collection advances until the key
# changes; an implementation that failed to advance in either place would emit
# output forever. That is the specific failure this timeout exists for, and it
# is why the *reference* is wrapped too: a harness that only bounded our side
# would hang on the day the reference was the buggy one.
run_ours() { timeout -k 2 30 "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; timeout -k 2 30 wsl -e env "LC_ALL=$loc" join "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands on
# the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'a 1\n' > .probe1
printf 'a x\n' > .probe2
if [ "$(run_gnu C .probe1 .probe2 2>/dev/null)" = "a 1 x" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "join-diff: glibc join not reachable in this directory; skipping"
fi
rm -f .probe1 .probe2

# --- fixtures ----------------------------------------------------------------
# The canonical pair: one key only in the first, one only in the second, two in
# both, so a single run exercises pairing and both unpairable sides.
printf 'a 1\nb 2\nd 4\n'          > a.txt
printf 'b x\nc y\nd z\n'          > b.txt
# Disjoint, so nothing ever pairs and the whole output is whatever -a and -v ask
# for.
printf 'p 7\nq 8\n'               > p.txt
# Empty: whichever side it is on, the other file is one long tail.
printf ''                         > empty.txt
# No final newline, which `readlinebuffer_delim` supplies — so the last line
# pairs with the same key elsewhere rather than being a different key.
printf 'a 1\nz 9'                 > unterm.txt
# A single unterminated line, so the first record read is also the last.
printf 'solo 1'                   > solo.txt
# Out of order. Only matters once something fails to pair, which is the entire
# subtlety of the default check.
printf 'c 3\na 1\nb 2\n'          > dis.txt
printf 'a 1\nb 2\nc 3\n'          > sorted.txt
# Descending throughout: several descents, but at most one warning per file.
printf 'z 1\ny 2\nx 3\nw 4\n'     > desc.txt
# Runs on both sides in unequal numbers: join emits the cross product, so 3x2
# lines come out of 5 lines in.
printf 'a 1\na 2\na 3\nb 9\n'     > dup1.txt
printf 'a x\na y\nb z\n'          > dup2.txt
# More fields than the join field, for -o and for the default "everything else"
# layout.
printf 'k1 f2 f3 f4\nk2 g2 g3\n'  > wide.txt
printf 'k1 F2 F3\nk2 G2 G3 G4\n'  > wide2.txt
# The key in field 2, for -1/-2/-j.
printf 'x k1 p\ny k2 q\n'         > key2.txt
printf 'k1 r\nk2 s\n'             > key1.txt
# Leading and repeated blanks, which the default splitter skips and collapses —
# and which the output does *not* reproduce, since the output separator is a
# single space.
printf '  a   1   2  \n b 2\n'    > blanks.txt
printf 'a x\nb y\n'               > plain.txt
# Case, for -i. Sorted for the *case-insensitive* order, which is what -i's own
# order check wants.
printf 'A 1\nb 2\nC 3\n'          > upper.txt
printf 'a x\nB y\nc z\n'          > mixed.txt
# Explicit tabs: with -t every occurrence separates, so a doubled tab makes an
# empty field that -e can fill.
printf 'a\t1\t2\nb\t\t4\nc\t5\n'  > tabs.txt
printf 'a\tx\nb\ty\nc\tz\n'       > tabs2.txt
# Colons, for a -t that is not whitespace.
printf 'a:1:2\nb::4\n'            > colons.txt
printf 'a:x\nb:y\n'               > colons2.txt
# A line with only the key, so the "rest of the line" is nothing at all and the
# separator that would precede it is not written.
printf 'a\nb 2\n'                 > short.txt
# Bytes that are not text, in the keys. Compared bytewise, which under `C` is
# what both sides do.
printf '\x80 1\n\xff 2\n'         > bad1.txt
printf '\x80 x\n\xfe y\n'         > bad2.txt
# NUL-separated, for -z. Note the newlines inside: under -z they are ordinary
# field separators, not record separators.
printf 'a 1\0b 2\0'               > nul1.txt
printf 'a x\0b y\0'               > nul2.txt
printf 'a 1\nb 2\0c 3\0'          > nulnl.txt
# Long enough to cross the reader's buffer boundary, and overlapping only in
# part so the merge keeps switching sides across that boundary.
{ for i in $(seq 1000 5999); do printf 'k%04d v%04d\n' "$i" "$i"; done; } > long1.txt
{ for i in $(seq 3000 7999); do printf 'k%04d w%04d\n' "$i" "$i"; done; } > long2.txt
mkdir subdir

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(join | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_gnu "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_gnu "$loc" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of the
  # getopt module is that the sentences match, so a harness that only asked "did
  # it complain?" would pass on every wording this exists to fix. It also
  # matters for the order check, which prints *at most one* warning per file and
  # names the file and line number in it.
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "join $*"; }
# The diagnostics that pass an argument through gnulib's quoting. They are
# already under `C` here — see the header — so this is a label, not a locale
# change: it marks the rows whose agreement would be at issue under B-Q2 if the
# rest of the harness ever moved to `C.UTF-8`.
run_ascii() { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "join $* [C]"; }
# The same case under a locale where GNU collates rather than compares bytes.
# Not a synonym for `run_case`: it is the measurement behind the claim that the
# divergence is confined to inputs whose collation order differs from their byte
# order. See the header.
run_utf8()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "join $* [C.UTF-8]"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C "$@"
  report "printf '$input' | join $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1"; shift
  compare - C "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS join %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL join %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default layout -------------------------------------------------------
# The shipped parser read `--`, `--header`, `--check-order` and the whole
# obsolescent `-j1 N` family as filenames, so a good many of these fail against
# it before any of the merge rules are involved.
run_case a.txt b.txt
run_case b.txt a.txt
run_case a.txt a.txt
run_case a.txt p.txt
run_case p.txt a.txt
run_case dup1.txt dup2.txt
run_case dup2.txt dup1.txt
run_case wide.txt wide2.txt
run_case wide2.txt wide.txt
run_case short.txt plain.txt
run_case plain.txt short.txt
run_case blanks.txt plain.txt
run_case bad1.txt bad2.txt
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
run_case unterm.txt sorted.txt
run_case sorted.txt unterm.txt
run_case unterm.txt unterm.txt
run_case unterm.txt empty.txt

# --- -a and -v ----------------------------------------------------------------
# An unpairable line prints its key first and then the rest of *its own* line,
# with no room left for the other file's fields — which is why -a's output is
# not a fixed number of columns.
run_case -a 1 a.txt b.txt
run_case -a 2 a.txt b.txt
run_case -a 1 -a 2 a.txt b.txt
run_case -a1 a.txt b.txt
run_case -a2 -a1 a.txt b.txt
run_case -a 1 -a 1 a.txt b.txt
run_case -v 1 a.txt b.txt
run_case -v 2 a.txt b.txt
run_case -v 1 -v 2 a.txt b.txt
run_case -v1 -a2 a.txt b.txt
run_case -a 1 -v 2 a.txt b.txt
run_case -v 2 -a 1 a.txt b.txt
run_case -a 1 p.txt a.txt
run_case -v 1 empty.txt b.txt
run_case -a 2 empty.txt b.txt
run_case -a 1 a.txt empty.txt
run_case -a 1 dup1.txt dup2.txt
run_case -v 1 dup1.txt dup2.txt
run_case -a 1 wide.txt wide2.txt
run_case -a 1 -a 2 long1.txt long2.txt
run_case -a 1 short.txt plain.txt

# --- -e, the empty-field filler -----------------------------------------------
# It replaces a field that is absent *and* a field that is present and empty,
# and only where the output would otherwise have written nothing.
run_case -e X -o 1.1,1.2,2.2 a.txt b.txt
run_case -e X -o 1.1,1.5,2.9 a.txt b.txt
run_case -e '' -o 1.1,2.9 a.txt b.txt
run_case -e X -t: -o 1.2,1.3,2.2 colons.txt colons2.txt
run_case -e X -a 1 -o 0,1.2,2.2 a.txt b.txt
run_case -e X -o auto a.txt b.txt
run_case -e X -o auto -a 1 wide.txt wide2.txt
run_case -eX -o 1.9 a.txt b.txt
run_case -e X -e X -o 1.9 a.txt b.txt
run_ascii -e X -e Y -o 1.9 a.txt b.txt
# Without -o it changes nothing: prfield only reaches the filler for fields the
# output list named.
run_case -e X a.txt b.txt

# --- -o -----------------------------------------------------------------------
run_case -o 0 a.txt b.txt
run_case -o 1.1 a.txt b.txt
run_case -o 2.2 a.txt b.txt
run_case -o 1.1,2.2 a.txt b.txt
run_case -o 0,1.2,2.2 a.txt b.txt
run_case -o '1.1 2.2' a.txt b.txt
run_case -o '1.1	2.2' a.txt b.txt
run_case -o 2.2,1.2,0 a.txt b.txt
run_case -o 1.2 -o 2.2 a.txt b.txt
# The obsolescent continuation: a *separate word* after -o extends the list, but
# only if enough operands follow to spare it. These are the rows that separate a
# transcription of `add_file_name` from an operand count taken at the end.
run_case -o 1.1 2.2 a.txt b.txt
run_case -o 0 1.2 2.2 a.txt b.txt
run_case -o 1.1 2.2 0 a.txt b.txt
run_case -o 1.1 a.txt b.txt
run_case -o 0 -a 1 1.2 a.txt b.txt
run_ascii -o 1.1 2.2 a.txt
run_ascii -o 1.1 a.txt b.txt c.txt
# -o pending and -j1 pending at once: the reinterpretation walks the slots from
# the front, so the *earlier* pending option claims the earlier name.
run_case -o 1.1 -j1 2 key2.txt key1.txt
run_case -j1 -o 1.1 2 key2.txt key1.txt
run_case -j1 2 -o 1.1 key2.txt key1.txt
run_case -o 1.9 a.txt b.txt
run_case -o 0,0,0 a.txt b.txt
run_case -o 0 -a 1 a.txt b.txt
run_case -o 0 -v 2 a.txt b.txt
run_case -o 1.1,1.2 -a 2 a.txt b.txt
run_case -o 0,1.2,2.2 dup1.txt dup2.txt
run_case -o 1.1,1.2,1.3,1.4 wide.txt wide2.txt
run_case -t: -o 1.1,1.2,1.3,2.2 colons.txt colons2.txt
# `auto` is the exact word, not a field spec, and it counts the fields of the
# *first* line of each file — so a later longer line is truncated and a shorter
# one padded.
run_case -o auto a.txt b.txt
run_case -o auto wide.txt wide2.txt
run_case -o auto wide2.txt wide.txt
run_case -o auto -a 1 wide.txt wide2.txt
run_case -o auto -a 2 wide.txt wide2.txt
run_case -o auto -v 1 wide.txt wide2.txt
run_case -o auto empty.txt b.txt
run_case -o auto a.txt empty.txt
run_case -o auto short.txt plain.txt
run_case -o auto -e Q wide.txt wide2.txt
run_case -o auto -t: colons.txt colons2.txt
# `auto` is not an abbreviation and not a prefix: `aut` and `autoX` are field
# specs, and bad ones.
run_ascii -o aut a.txt b.txt
run_ascii -o auto,1.1 a.txt b.txt
run_ascii -o 1.1,auto a.txt b.txt

# --- -t, which changes what a field is and how the output is spaced ------------
run_case -t: colons.txt colons2.txt
run_case -t: -a 1 colons.txt colons2.txt
run_case -t: -o 0,1.2,2.2 colons.txt colons2.txt
run_case -t: colons.txt colons.txt
run_case '-t	' tabs.txt tabs2.txt
run_case '-t	' -a 1 tabs.txt tabs2.txt
run_case '-t	' -e . -o 1.2,1.3 tabs.txt tabs2.txt
# A -t whose argument is empty is a newline, which no line can contain — so
# every line is exactly one field and the whole line is the key.
run_case -t '' plain.txt plain.txt
run_case -t '' a.txt a.txt
run_case -t '' a.txt b.txt
# The two-character `\0` is a NUL, which is the one multi-character argument
# that is not refused.
run_case -t '\0' plain.txt plain.txt
run_case -t '\0' nul1.txt nul2.txt
run_case -t: -t: colons.txt colons2.txt
run_ascii -t: -t, colons.txt colons2.txt
run_ascii -t xy a.txt b.txt
run_ascii -t '\1' a.txt b.txt
# A -t that is a space is not the default: it splits on every single space
# rather than on runs, so a doubled space makes an empty field.
run_case -t ' ' blanks.txt plain.txt
run_case -t ' ' a.txt b.txt

# --- -1, -2, -j and the obsolescent forms -------------------------------------
run_case -1 2 key2.txt key1.txt
run_case -2 2 key1.txt key2.txt
run_case -1 2 -2 2 key2.txt key2.txt
run_case -j 1 a.txt b.txt
run_case -j 2 wide.txt wide2.txt
run_case -12 key2.txt key2.txt
run_case -1 1 -2 1 a.txt b.txt
run_case -1 5 a.txt b.txt
run_case -j 5 a.txt b.txt
run_case -1 2 -o 0,1.1,2.1 key2.txt key1.txt
run_case -1 2 -a 1 key2.txt key1.txt
# The obsolescent `-j1 N` and `-j2 N`: the operand after the option may turn out
# to be the option's argument, decided by how many operands are left at the end.
run_case -j1 2 key2.txt key1.txt
run_case -j2 2 key1.txt key2.txt
run_case -j1 2 -j2 2 key2.txt key2.txt
run_case -j1 3 a.txt b.txt
run_case -j1 a.txt b.txt
run_case -j2 a.txt b.txt
run_case -j1 1 a.txt b.txt
# Attached to a cluster it is not the ambiguous form, because the argument does
# not start two bytes into the word.
run_case -ij1 a.txt b.txt
run_ascii -j 1 -1 2 a.txt b.txt
run_ascii -1 2 -j 1 a.txt b.txt
run_ascii -j1 2 -1 3 key2.txt key1.txt
run_ascii -j 0 a.txt b.txt
run_ascii -1 0 a.txt b.txt
run_ascii -2 -1 a.txt b.txt
run_ascii -1 x a.txt b.txt
run_ascii -1 1x a.txt b.txt
run_ascii -1 '' a.txt b.txt
# Overflow is clamped to PTRDIFF_MAX rather than refused, so a field number no
# file could have is accepted and every key is empty.
run_case -1 99999999999999999999 a.txt b.txt
run_case -j 99999999999999999999 a.txt b.txt
run_ascii -1 99999999999999999999x a.txt b.txt
run_ascii -1 -9223372036854775808 a.txt b.txt
run_case -1 -9223372036854775809 a.txt b.txt

# --- -i -----------------------------------------------------------------------
run_case -i upper.txt mixed.txt
run_case -i mixed.txt upper.txt
run_case upper.txt mixed.txt
run_case -i -a 1 upper.txt mixed.txt
run_case --ignore-case upper.txt mixed.txt
run_case --ign upper.txt mixed.txt
# The tiebreak is length, after the fold — a key that is a prefix of another is
# still smaller.
printf 'ab 1\nabc 2\n' > pre1.txt
printf 'AB x\nABCD y\n' > pre2.txt
run_case -i pre1.txt pre2.txt
run_case pre1.txt pre2.txt

# --- --header -----------------------------------------------------------------
# The first line of each file is paired unconditionally, whatever the keys say,
# and it is not compared against anything for order.
run_case --header a.txt b.txt
run_case --header dis.txt sorted.txt
run_case --header empty.txt b.txt
run_case --header a.txt empty.txt
run_case --header empty.txt empty.txt
run_case --header -a 1 a.txt b.txt
run_case --header -o 0,1.2,2.2 a.txt b.txt
run_case --header -o auto wide.txt wide2.txt
run_case --header -t: colons.txt colons2.txt
run_case --header --check-order dis.txt sorted.txt
run_case --head a.txt b.txt

# --- the order check ----------------------------------------------------------
# Three states, and they are not "warn / warn / quiet". By default the check is
# armed only once some line has failed to pair, so a disordered pair of files
# whose lines all pair is silent.
run_case dis.txt sorted.txt
run_case sorted.txt dis.txt
run_case dis.txt dis.txt
run_case dis.txt a.txt
run_case a.txt dis.txt
run_case desc.txt sorted.txt
run_case desc.txt desc.txt
run_case --check-order dis.txt sorted.txt
run_case --check-order sorted.txt dis.txt
run_case --check-order dis.txt dis.txt
run_case --check-order desc.txt desc.txt
run_case --check-order a.txt b.txt
run_case --check-order sorted.txt sorted.txt
run_case --nocheck-order dis.txt sorted.txt
run_case --nocheck-order dis.txt dis.txt
run_case --nocheck-order desc.txt desc.txt
# The last one wins, in both directions.
run_case --check-order --nocheck-order dis.txt sorted.txt
run_case --nocheck-order --check-order dis.txt sorted.txt
run_case --check --nocheck dis.txt sorted.txt
run_case --noc dis.txt sorted.txt
run_case --check dis.txt sorted.txt
# The warning names the file and the line, and the line is printed raw. With -z
# the record holds newlines and ends at a NUL, and `%.*s` stops at the first one
# — so the warning shows only what precedes it.
run_case -z --check-order nulnl.txt nul2.txt
run_case --check-order bad1.txt bad2.txt
# -a and -v change *when* the default check arms, because they change nothing
# about pairing — but -v 1 makes the unpairable lines visible beside the
# warning, which is the interleaving of stdout and stderr this checks.
run_case -a 1 dis.txt sorted.txt
run_case -v 1 dis.txt sorted.txt
run_case --check-order -o 0 dis.txt sorted.txt
# The tail is drained for the check even when nothing more will be printed.
run_case --check-order sorted.txt solo.txt
run_case --check-order desc.txt solo.txt

# --- -z, which changes what a line is -----------------------------------------
run_case -z nul1.txt nul2.txt
run_case -z nul2.txt nul1.txt
run_case -z nulnl.txt nul2.txt
run_case -z a.txt b.txt
run_case -z empty.txt nul1.txt
run_case -z nul1.txt empty.txt
run_case -z -a 1 nul1.txt nul2.txt
run_case -z -o 0,1.2,2.2 nul1.txt nul2.txt
run_case -z -t: colons.txt colons2.txt
run_case --zero-terminated nul1.txt nul2.txt
run_case --zero nul1.txt nul2.txt
run_case -iz nul1.txt nul2.txt
run_case -zi nul1.txt nul2.txt

# --- standard input -----------------------------------------------------------
run_stdin 'a 1\nb 2\nd 4\n' - b.txt
run_stdin 'b x\nc y\nd z\n' a.txt -
run_stdin 'a 1\nb 2\nd 4\n' -a 1 - b.txt
run_stdin '' - a.txt
run_stdin 'a 1\nz 9' - sorted.txt
run_stdin 'c 3\na 1\nb 2\n' - sorted.txt
run_stdin 'a 1\nb 2\n' -o auto - b.txt
# Both operands as stdin is refused outright, before either is read.
run_stdin 'a 1\nb 2\n' - -

# --- operands are counted exactly ---------------------------------------------
# Fewer than two is one of two sentences, and the one-operand sentence names the
# operand — which, since join does not permute, is whatever came last.
run_ascii
run_ascii a.txt
run_ascii -i a.txt
run_ascii a.txt -i
run_ascii nosuch.txt
run_ascii -
run_ascii a.txt b.txt empty.txt
run_ascii a.txt b.txt empty.txt sorted.txt
run_ascii -i a.txt b.txt empty.txt
# With the obsolescent -j1 in play the *last* operand may be reclaimed as an
# argument, so the same number of words can be right or wrong depending on it.
run_ascii -j1 2
run_ascii -j1 2 a.txt
run_ascii -j1 2 a.txt b.txt empty.txt
run_ascii -o 1.1 a.txt b.txt empty.txt

# --- operands that cannot be opened -------------------------------------------
# The first file is opened first, so a pair of bad ones names only the first.
run_ascii nosuch.txt nosuch2.txt
run_ascii a.txt nosuch.txt
run_ascii nosuch.txt a.txt
run_ascii -a 1 nosuch.txt a.txt
# The parse is finished before anything is opened, so a bad option wins.
run_ascii -Q nosuch.txt nosuch2.txt
run_ascii -t: -t, nosuch.txt nosuch2.txt

# --- getopt diagnostics -------------------------------------------------------
run_ascii -Q a.txt b.txt
run_ascii -iQ a.txt b.txt
run_ascii -Qi a.txt b.txt
run_ascii -3 a.txt b.txt
run_ascii -0 a.txt b.txt
run_ascii --nope a.txt b.txt
run_ascii --ignore-case=x a.txt b.txt
run_ascii --check-order=x a.txt b.txt
run_ascii --nocheck-order=x a.txt b.txt
run_ascii --zero-terminated=x a.txt b.txt
run_ascii --header=x a.txt b.txt
run_ascii --help=x a.txt b.txt
run_ascii --version=x a.txt b.txt
run_ascii --=x a.txt b.txt
run_ascii -a
run_ascii -e
run_ascii -o
run_ascii -t
run_ascii -1
run_ascii -j
run_ascii -a 3 a.txt b.txt
run_ascii -a 0 a.txt b.txt
run_ascii -a x a.txt b.txt
run_ascii -v 3 a.txt b.txt
run_ascii -a '' a.txt b.txt
run_ascii -- -Q
# The field-spec sentences: three of them, chosen by which part is wrong.
run_ascii -o 3.1 a.txt b.txt
run_ascii -o 0.1 a.txt b.txt
run_ascii -o 1 a.txt b.txt
run_ascii -o 1. a.txt b.txt
run_ascii -o 1.0 a.txt b.txt
run_ascii -o 1.x a.txt b.txt
run_ascii -o x a.txt b.txt
run_ascii -o '' a.txt b.txt
run_ascii -o 1.1, a.txt b.txt
run_ascii -o ,1.1 a.txt b.txt
run_ascii -o '1.1 ' a.txt b.txt
run_ascii -o 1.1,,2.2 a.txt b.txt
run_ascii -o 00 a.txt b.txt
run_ascii -o 0,x a.txt b.txt
# A getopt error beats a bad operand count, and both beat a missing file.
run_ascii -Q
run_ascii -Q a.txt
run_ascii --nope

# --- abbreviations, which the shipped parser did not accept at all ------------
run_case --i upper.txt mixed.txt
run_case --z nul1.txt nul2.txt
run_case --zero-t nul1.txt nul2.txt
run_case --hea a.txt b.txt
run_ascii --c dis.txt sorted.txt
run_ascii --n dis.txt sorted.txt
run_ascii --ch dis.txt sorted.txt
run_ascii --he a.txt b.txt

# --- options and operands interleave, and are not permuted --------------------
run_case a.txt -i b.txt
run_case a.txt b.txt -i
run_case -- a.txt b.txt
run_case -i -- a.txt b.txt
run_case a.txt -- b.txt
run_case -a 1 -- a.txt b.txt
run_case -- -j1 2
run_case a.txt -a 1 b.txt

# --- how far the byte comparison and GNU's collation agree --------------------
# Under `C.UTF-8` GNU switches to `xmemcoll`. For these inputs codepoint order
# and byte order coincide, so the two still agree; the rows exist to catch the
# day that stops being true, and to bound the claim in the header. The
# non-text-byte fixtures are deliberately absent: an invalid UTF-8 sequence has
# no codepoint to collate, and what glibc does with it is not a rule we mean to
# reproduce.
run_utf8 a.txt b.txt
run_utf8 -a 1 -a 2 a.txt b.txt
run_utf8 dup1.txt dup2.txt
run_utf8 pre1.txt pre2.txt
run_utf8 long1.txt long2.txt
run_utf8 -i upper.txt mixed.txt

# --- differ on purpose --------------------------------------------------------
# On SlateOS, and on any POSIX host, opening a directory succeeds and the *read*
# fails — so GNU reaches `join: read error: Is a directory`, which names no file
# at all. This harness runs a Windows build, where `File::open` of a directory
# fails outright and the errno is the host's. The difference is the host's, not
# the code's — see the same trap in `comm-diff.sh` and in `filekind.rs`.
xfail_case 'a directory operand cannot be opened on a Windows host' subdir a.txt
xfail_case 'a directory operand cannot be opened on a Windows host' a.txt subdir

xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are here too, and as xfails rather than as ordinary
# cases: they reach the same two texts, so a `run_case` row for them would
# report the *body* difference we already accept and hide the thing they are
# actually for — that `--h` and `--v` resolve at all rather than being rejected
# as ambiguous against `--header`. `--h` is in fact ambiguous, so it is an
# ordinary case; only `--v` is unique.
run_ascii --h a.txt b.txt
xfail_case 'an abbreviation of --version reaches our version text' --v a.txt b.txt

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1

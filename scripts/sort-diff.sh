#!/usr/bin/env bash
# Differential test: our sort against the host's GNU sort.
#
# `sort` is the utility where "it looks right" is least trustworthy. Every
# wrong answer it can give is still a permutation of the input, so a wrong
# comparison produces output that reads perfectly and is simply in the wrong
# order — there is nothing to notice. Unit tests do not help either, because a
# test written next to the code asserts the same belief the code holds. The
# only way to find a misreading of the specification is to put the same bytes
# through the implementation everyone means when they say `sort` and require
# the two to agree.
#
# Comparison is on a hex dump of stdout, on the exit status, and on *whether*
# there was a diagnostic. The hex dump matters here for the same reason it does
# in cat-diff.sh: `$(...)` strips trailing newlines and eats NULs, and `-z`
# output is nothing but NULs. stderr wording is not compared — GNU's comes from
# its getopt and the host's error table, ours from `coreutils::errmsg`.
#
# ## Why both sides run inside WSL, and why that let this file lose half of
# itself
#
# `scripts/diff-wsl.sh` gives the general reasons. This harness is the one that
# proved them: the "option parsing" section below spent a while certifying
# wording no GNU/Linux system has ever printed, because `$GNU` was MSYS2's sort
# and MSYS2's getopt is not glibc's. See that section, and `known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`.
#
# The fix at the time was local: give that one section a second reference
# (`wsl -e env LC_ALL=C sort`), and a third for the `argmatch` rows that need a
# UTF-8 locale — three references, two "is it reachable" probes, and a
# `HAVE_GLIBC` guard on every row so the file still ran on a host with only
# MSYS. All of that was scaffolding for a split that no longer exists. There is
# one reference now, it is glibc's, and every row uses it.
#
# ## The locale is `C.UTF-8`, not `C`
#
# The reason this file used to pin `C` still holds: in a collating locale GNU
# sorts with the locale's tables and SlateOS has none (`known-issues.md`), so
# comparing against a collating GNU would be comparing against a specification
# we have deliberately not implemented. `C.UTF-8` is not a collating locale —
# glibc gives it codepoint order, the same as `C`. Measured, not assumed: every
# fixture in this file, under every ordering option it uses, produces
# byte-identical output from glibc's sort under the two.
#
# What `C.UTF-8` additionally buys is the `argmatch` rows, which used to need a
# reference of their own: gnulib's `quote()` prints U+2018/U+2019 under a UTF-8
# locale and ASCII apostrophes under `C`, and since §351 ours prints the curly
# pair in every locale. Under `C` the *reference* was the wrong one.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one name
# `sort` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=sort
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Both sides are reached through a symlink named `sort` in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word on
# both and the `sort: ` prefix on every diagnostic matches.
OURS_RUN="env PATH=$bindir/ours sort"
GNU_RUN="env PATH=$bindir/gnu sort"

echo "sort-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# Two columns, blanks of both widths, so a field's leading blanks are visible.
printf 'b 2\na 10\nc 1\nd  3\n e 2\n'                   > cols.txt
# Ties on every key, so the last-resort comparison is what decides.
printf 'x 1\nc 1\na 1\nb 1\n'                           > ties.txt
# The spellings of one number, which -n must tie and -u must then collapse.
printf '1\n1.0\n01\n1.00\n+1\n-0\n0\n'                  > spellings.txt
# Wider than a double can hold exactly.
printf '18446744073709551616\n18446744073709551617\n18446744073709551616.5\n' > big.txt
printf '1e3\n999\n0x10\n17\n+5\n-inf\nnan\ninf\n.5\n'   > general.txt
printf '2K\n1M\n900\n1024K\n3M\n-1M\n1G\n0K\n0M\n'      > human.txt
printf 'Jan\nDEC\nfeb 2\nmarch\nxyz\n\napr\n'           > months.txt
printf '1.10\n1.9\n1.0~rc1\n1.0\n1.0.1\n2\n1.0~rc2\n'   > versions.txt
printf 'a:2:z\nb:1:y\nc:10:x\n::w\na::v\n'              > colons.txt
printf 'Apple\nbanana\nApricot\nBanana\napple\n'        > case.txt
printf 'a-b\nab\na b\nA_B\n'                            > punct.txt
printf 'b\na\x01c\nab\n'                                > control.txt
printf ''                                               > empty.txt
printf 'only'                                           > unterminated.txt
printf 'a\nb\nc\n'                                      > sorted.txt
printf 'c\nb\na\n'                                      > unsorted.txt
printf 'a\na\nb\n'                                      > dupes.txt
printf 'a\nc\ne\n'                                      > merge1.txt
printf 'b\nd\nf\n'                                      > merge2.txt
printf '3\n1\n'                                         > mergebad1.txt
printf '2\n'                                            > mergebad2.txt
# Not valid UTF-8. The old sort stopped at this file with a diagnostic.
printf 'a\n\xff\nb\n\xc3\xa9\n\x80\n'                   > bytes.txt
printf 'b\x00a\x00c\x00'                                > nul.txt

compare() {
  local o_out g_out o_err g_err o_bin g_bin o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file, not through a pipe into `od`: in `x=$(sort | od)`
  # the status recorded would be od's, so every failing case would pass.
  if [ "$stdin" = "-" ]; then
    $OURS_RUN "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $GNU_RUN  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | $OURS_RUN "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $GNU_RUN  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_loud" = "$g_loud" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$o_err")" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$g_err")")
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

run_case()  { compare - "$@"; report "sort $*"; }
run_stdin() { local input="$1"; shift; compare "$input" "$@"; report "printf '$input' | sort $*"; }

xfail_case() {
  local reason="$1"; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL sort %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS sort %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# --- the plain sort ---------------------------------------------------------
run_case cols.txt
run_case ties.txt
run_case empty.txt
run_case unterminated.txt
run_case sorted.txt unsorted.txt
run_case -r cols.txt
run_case bytes.txt
run_case -r bytes.txt
run_stdin 'b\na'
run_stdin ''
run_stdin '\n\n\n'
run_stdin 'a\n\n b\n\tb\n'

# --- -n, and the ways it is not a general number parser ---------------------
run_case -n spellings.txt
run_case -nr spellings.txt
run_case -n big.txt
run_case -n general.txt
run_case -n cols.txt
run_case -n months.txt
run_stdin '2\n10\n-3\n \n' -n
run_stdin '.5\n0.50\n-.5\n' -n
run_stdin '1e3\n2\n' -n
run_stdin '0x10\n1\n' -n
run_stdin '+5\n1\n' -n

# --- -u, which is unique by key and not by line -----------------------------
run_case -u dupes.txt
run_case -nu spellings.txt
run_case -u spellings.txt
run_case -u ties.txt
run_case -k2,2 -u cols.txt
run_case -k1,1 -u ties.txt
run_case -ur dupes.txt

# --- -s, which turns the last resort off ------------------------------------
run_case -s -k2,2 cols.txt
run_case -s -k1,1 ties.txt
run_case -s -n cols.txt
run_case -s cols.txt

# --- -k: fields, offsets, and the blanks in front of them -------------------
run_case -k2 cols.txt
run_case -k2,2 cols.txt
run_case -k1,1 cols.txt
run_case -k2,2n cols.txt
run_case -k2n cols.txt
run_case -k1.2 cols.txt
run_case -k1.2,2.1 cols.txt
run_case -k2,2 -k1,1 cols.txt
run_case -k1,1 -k2,2n cols.txt
run_case -k3,3 cols.txt
run_case -k9,9 cols.txt
run_case -k2,1 cols.txt
run_case -k1 cols.txt
run_case -b -k2,2 cols.txt
run_case -k2b,2 cols.txt
run_case -k1b,1 cols.txt
run_case -k1,1b cols.txt
run_case -k2,2b cols.txt
run_case -r -k2,2 cols.txt
run_case -r -k2,2n cols.txt
run_case -r -n -k2,2 cols.txt
run_case -k2,2r cols.txt
run_case --key=2,2 cols.txt
run_case -k 2,2 cols.txt

# --- -t: the separator belongs to no field ----------------------------------
run_case -t: -k2,2 colons.txt
run_case -t: -k2,2n colons.txt
run_case -t: -k1,1 colons.txt
run_case -t: -k3,3 colons.txt
run_case -t: colons.txt
run_case -t: -k2 colons.txt
run_case -t ' ' -k1,1 cols.txt
run_case -t ' ' -k2,2 cols.txt
run_case --field-separator=: -k2,2 colons.txt
run_stdin 'a\tb\nb\ta\n' -t '\t' -k2,2

# --- -f -d -i: the transformed default ordering -----------------------------
run_case -f case.txt
run_case -fu case.txt
run_case -d punct.txt
run_case -i control.txt
run_case -df punct.txt
run_case -k1,1f case.txt
run_case -k1,1d punct.txt
run_case --ignore-case case.txt
run_case --dictionary-order punct.txt

# --- -g, -h, -M, -V ---------------------------------------------------------
run_case -g general.txt
run_case -g big.txt
run_case -gr general.txt
run_case -h human.txt
run_case -hr human.txt
run_case -M months.txt
run_case -Mr months.txt
run_case -V versions.txt
run_case -Vr versions.txt
run_case -k2,2V colons.txt
run_case --version-sort versions.txt
run_case --human-numeric-sort human.txt
run_case --month-sort months.txt
run_case --general-numeric-sort general.txt

# --- -z ---------------------------------------------------------------------
run_case -z nul.txt
run_case -zu nul.txt
run_case -z empty.txt
run_stdin 'b\x00a\x00' -z

# --- -c and -C --------------------------------------------------------------
run_case -c sorted.txt
run_case -c unsorted.txt
run_case -C sorted.txt
run_case -C unsorted.txt
run_case -cu dupes.txt
run_case -c dupes.txt
run_case -c empty.txt
run_case -c -k2,2n cols.txt
run_case --check sorted.txt
run_case --check=quiet unsorted.txt
run_case -c sorted.txt unsorted.txt

# --- -m ---------------------------------------------------------------------
run_case -m merge1.txt merge2.txt
run_case -mu merge1.txt merge1.txt
run_case -m mergebad1.txt mergebad2.txt
run_case -m -r merge1.txt merge2.txt
run_case -m empty.txt merge1.txt
run_case --merge merge1.txt merge2.txt

# --- the obsolete key syntax ------------------------------------------------
run_case +1 cols.txt
run_case +1 -2 cols.txt
run_case +0.1 cols.txt
run_case +1n cols.txt

# --- accepted and ignored ---------------------------------------------------
run_case -S 1M cols.txt
run_case -T . cols.txt
run_case --parallel=2 cols.txt
run_case --buffer-size=1M cols.txt

# --- failure, and the exit status that reports it ---------------------------
run_case nosuchfile.txt
run_case -k0 cols.txt
run_case -k1.0 cols.txt
run_case -kx cols.txt
run_case -k1x cols.txt
run_case -k1,0 cols.txt
run_case -k1,x cols.txt
run_case -k1. cols.txt
run_case -k1n,2M cols.txt
run_case -t ab cols.txt
run_case -t: -t';' cols.txt
run_case -Q cols.txt
run_case --nope cols.txt
run_case -c sorted.txt unsorted.txt

# --- second wave: the combinations, which is where the corners are ----------
# Several keys, so the fall-through from one to the next is exercised.
run_case -k1,1 -k2,2n cols.txt
run_case -k2,2n -k1,1r cols.txt
run_case -k2,2 -k1,1 -k2,2n cols.txt
run_case -t: -k3,3 -k2,2n colons.txt
run_case -t: -k2,2n -k1,1 colons.txt
run_case -u -k2,2n cols.txt
run_case -s -k2,2n -k1,1 cols.txt

# A key that starts or ends past the end of the line.
run_case -k1.9 cols.txt
run_case -k1.9,1.20 cols.txt
run_case -k9.9,9.9 cols.txt
run_case -k1,9 cols.txt
run_case -k1.3,1.4 cols.txt
run_case -t: -k1.2,1.3 colons.txt
run_case -t: -k2.1,2.2 colons.txt

# `b` interacting with an offset, which is the only case where it does
# anything at the end position.
run_case -k2b,2.1 cols.txt
run_case -k1,2.1b cols.txt
run_case -k1b,2.1b cols.txt
run_case -b -k1.2,2.1 cols.txt

# The transformations applied to a key rather than globally.
run_case -k1,1i control.txt
run_case -k1,1fd case.txt
run_case -f -k1,1 case.txt
run_case -i -u control.txt
run_case -d -u punct.txt
run_case -fu -k1,1 case.txt

# -n and friends on text that is not a number.
run_stdin 'abc\ndef\n' -n
run_stdin 'abc\ndef\n' -g
run_stdin 'abc\ndef\n' -h
run_stdin '  \n\t\n' -n
run_stdin '-\n+\n.\n' -n
run_stdin '-\n+\n.\n' -g
run_stdin '1.2.3\n1.2\n' -n
run_stdin '1,000\n999\n' -n
run_stdin '00000000000000000001\n1\n' -n
run_stdin '-0.0\n0.0\n0\n' -n
run_stdin '1e400\n1e-400\n1\n' -g
run_stdin '0x1p4\n16\n15\n' -g
run_stdin 'INF\n-INF\nNAN\n1\n' -g
run_stdin '1K\n1k\n1KB\n' -h
run_stdin '0.5M\n500K\n' -h
run_stdin '1.5K\n1500\n' -h

# Month names that are almost month names.
run_stdin 'JANUARY\njan\nJa\nJUN\nJUL\n' -M
run_stdin '  DEC\n\tdec\n' -M
run_stdin 'mayonnaise\nmay\n' -M

# Version strings that exercise filevercmp's odd corners.
run_stdin 'a1\na01\na001\n' -V
run_stdin '1.0-1\n1.0.1\n1.0~1\n' -V
run_stdin 'foo\nfoo~\n~foo\n' -V
run_stdin 'a.b\na-b\na_b\n' -V
run_stdin '\n1\na\n~\n' -V

# filevercmp is a *file name* comparison: dot files come first, and the file
# suffix is cut off before the stems are compared. These are the cases that
# distinguish it from a plain version-string compare.
run_stdin '.b\nb\n.\n..\n\n~\n' -V
run_stdin '.bashrc\n.bash_profile\nbashrc\n.\n' -V
run_stdin 'x\nx.tar\nx.tar.gz\nx.tar.bz2\n' -V
run_stdin 'foo.c\nfoo.h\nfoo\nfoo-1.c\n' -V
run_stdin 'x.1\nx.2\nx.9\nx.10\n' -V
run_stdin 'a.b~\na.b\na.~\na~\n' -V
run_stdin 'lib.so.1\nlib.so.10\nlib.so.2\nlib.so\n' -V
run_stdin 'v1.0\nv1.0rc1\nv1.0~rc1\nv1.0.0\n' -V
# The suffix must start with a letter or `~`, so `.2gz` is not one.
run_stdin 'f.2gz\nf.gz2\nf.tgz\n' -V
run_stdin '..a\n.a\na\n...\n' -V

# Empty lines and lines that are only blanks, under every ordering.
run_stdin '\n \nb\n\n' -b
run_stdin '\n \nb\n\n' -n
run_stdin '\n \nb\n\n' -k1,1
run_stdin '\n \nb\n\n' -u
run_stdin '\n \nb\n\n' -d

# -z with keys and fields.
run_stdin 'b 2\x00a 10\x00' -z -k2,2n
run_stdin 'b\x00a\x00b\x00' -zu
run_stdin 'b\x00a' -z

# -o, including the case that made the whole input be read up front.
run_case -o out1.txt unsorted.txt
run_case -o unsorted.txt unsorted.txt
run_case --output=out2.txt unsorted.txt
run_case -o /nonexistent-dir/x unsorted.txt

# Options after operands, and `--`.
run_case unsorted.txt -r
run_case -- unsorted.txt
run_case unsorted.txt --
run_case -n -- spellings.txt
run_case - -r
run_stdin 'b\na\n' -

# Bundles, and a value attached to its letter.
run_case -rn spellings.txt
run_case -nru spellings.txt
run_case -sk2,2 cols.txt
run_case -uk1,1 ties.txt
run_case -rk2,2n cols.txt
run_case -zru nul.txt

# Non-UTF-8 bytes everywhere a comparison could try to decode them.
run_case -u bytes.txt
run_case -n bytes.txt
run_case -f bytes.txt
run_case -d bytes.txt
run_case -i bytes.txt
run_case -V bytes.txt
run_case -k1,1 bytes.txt
run_stdin '\xff\n\xfe\n' -r


# --- option parsing, stderr text included -----------------------------------
#
# Everywhere else this script compares only *whether* stderr was loud, because
# the wording of an I/O error comes from the host's error table. Option
# diagnostics are different: they are ours to get right, and they are the whole
# of what a mistyped command line produces. So this section compares the text.
#
# It exists because a battery of these found five real defects at once: long
# options did not take a value from the next argument (`--key 2` was refused),
# did not accept unambiguous abbreviations (`--rev` was refused), reported the
# wrong wording for four different mistakes, exited 2 where GNU exits 1 for a
# bad argument *to* an option, and were missing `--sort`, `--files0-from`,
# `--random-sort` and `--debug` from the table entirely — so `--d`, which GNU
# calls ambiguous, silently resolved to `--dictionary-order`.
#
# ...and it then spent a while measuring the wrong thing, which is worth more
# than the defects it found. `$GNU` was MSYS2's `sort`, and MSYS2 is a Cygwin
# derivative: it links `msys-2.0.dll` rather than glibc, and **its getopt is not
# glibc's**. The two disagree on every message in this section:
#
#     command      msys-2.0 (coreutils 8.32)          glibc (coreutils 9.4)
#     sort -x      unknown option -- x                invalid option -- 'x'
#     sort --bogus unknown option -- bogus            unrecognized option '--bogus'
#     sort --s     ambiguous option -- s              option '--s' is ambiguous; …
#     sort --key   option requires an argument -- key option '--key' requires an argument
#     sort --rev=x option doesn't take an argument …  option '--rev' doesn't allow an argument
#
# So these cases all *passed* while our sort emitted wording no GNU/Linux system
# has ever printed. A differential harness is only as good as the thing it
# differs against, and "GNU sort" on this host turned out to be two different
# programs.
#
# The first fix was local — a second reference for this section alone — and it
# was the wrong shape, because it left the file with two ideas of what GNU is
# and a `HAVE_GLIBC` guard on every row to cope. `scripts/diff-wsl.sh` is that
# fix generalised: the whole harness now references glibc, so this section is
# ordinary and needs nothing of its own.

# Compare stderr and status, rather than only *whether* stderr was loud.
run_msg() {
  local o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  printf 'a\n' | $OURS_RUN "$@" >/dev/null 2>"$o_err"; o_rc=$?
  printf 'a\n' | $GNU_RUN  "$@" >/dev/null 2>"$g_err"; g_rc=$?
  if [ "$o_rc" = "$g_rc" ] && cmp -s "$o_err" "$g_err"; then
    AGREED=yes
  else
    AGREED=no
    REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s' \
      "$o_rc" "$(tr '\n' '|' <"$o_err")" "$g_rc" "$(tr '\n' '|' <"$g_err")")
  fi
  rm -f "$o_err" "$g_err"
  report "sort $* [stderr]"
}

# The cases where differing from glibc is the point. `report` is bypassed so a
# difference counts as expected and, more usefully, so agreement is reported as
# an XPASS — if we ever stop escaping, this says so instead of going quiet.
xfail_msg() {
  local reason="$1"; shift
  local o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  printf 'a\n' | $OURS_RUN "$@" >/dev/null 2>"$o_err"; o_rc=$?
  printf 'a\n' | $GNU_RUN  "$@" >/dev/null 2>"$g_err"; g_rc=$?
  if [ "$o_rc" = "$g_rc" ] && cmp -s "$o_err" "$g_err"; then
    xpass=$((xpass+1))
    printf 'XPASS sort %s [stderr]\n  now agrees with glibc, so this reason is stale: %s\n' \
      "$*" "$reason"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL sort %s [stderr]  (%s)\n' "$*" "$reason"
  fi
  rm -f "$o_err" "$g_err"
  return 0
}

# Abbreviation: unambiguous ones resolve, ambiguous ones are refused, and an
# exact match wins even when it is a prefix of a longer option.
run_msg --rev
run_msg --r
run_msg --d
run_msg --c
run_msg --che
run_msg --k
run_msg --m
run_msg --s
run_msg --u
run_msg --i
run_msg --z
run_msg --b
run_msg --h
run_msg --v
run_msg --n
run_msg --g
run_msg --co
run_msg --fie
run_msg --stab
# The ambiguous list is printed in the order the options are *declared*, not
# alphabetically, so it is a direct readout of GNU's table. An empty prefix
# matches everything, which makes `--=x` print the whole table in one line and
# is how that order was measured in the first place.
run_msg --=x
# The five getopt diagnostics. A short option and a long one get different
# sentences, and the two that resolve to something name the resolution rather
# than what was typed — `--k` reports `--key`, `--stab=x` reports `--stable`.
run_msg -x
run_msg -k
run_msg -o
run_msg --bogus
run_msg --fo=bar
run_msg --key
run_msg --output
run_msg --sort
run_msg --rev=x
run_msg --stab=x
run_msg --zero-term=x
run_msg --help=x
run_msg --version=x
run_msg --parallel
run_msg --field-separator
# A byte that is neither an option nor printable. This is the one place we
# differ from glibc deliberately: glibc writes the byte between two literal `'`
# and escapes nothing between them, so the diagnostic carries a raw control byte
# — and for a *long* option, whose name is arbitrary-length, a raw newline, which
# lets a file called `--fo\nsort: ...` forge a second diagnostic line. We put the
# name through `quote` instead. Every name a person would type is unaffected,
# which is what the cases above check.
xfail_msg "glibc emits the raw byte; we escape it" -$'\xc3'
xfail_msg "glibc emits the raw byte; we escape it" -$'\x01'
# argmatch: a bad argument *to* an option lists the valid ones and exits 1. It
# is a prefix match like getopt's, and an ambiguous one is a different sentence
# from an invalid one — but only when the candidates disagree, which is why
# `--check=q` resolves while `--check=` does not.
#
# These are the only rows in this section that do *not* come from glibc's
# getopt, and they used to need a reference of their own: `argmatch` is
# gnulib's and quotes with `quote()`, which since §351 is curly on our side in
# every locale and is curly on GNU's only under a UTF-8 one. Now that the whole
# file runs at `C.UTF-8` — see the locale note in the header — they are just
# rows.
run_msg --sort=bogus
run_msg --check=bogus
run_msg --check=
run_msg --sort=
run_msg --check=quiets
run_msg --sort=NUMERIC
run_stdin '10\n9\n' --sort=hum
run_stdin '10\n9\n' --sort=n
run_stdin 'b\na\n' --check=q
run_stdin 'b\na\n' --check=d
# `--files0-from` and its refusals.
run_msg --files0-from=no-such-list
printf 'sorted.txt\0' > names0
printf 'sorted.txt\0\0sorted.txt\0' > names0-empty
: > names0-none
run_case --files0-from=names0
run_msg --files0-from=names0-empty
run_msg --files0-from=names0-none
run_msg --files0-from=names0 sorted.txt
# A value may be written either way round.
run_stdin 'b 1\na 2\n' --key 2
run_stdin 'b 1\na 2\n' --key=2
run_stdin '10\n9\n' --sort numeric
run_stdin '10\n9\n' --sort=numeric
run_stdin 'a\nb\n' --field-separator , -k1
# `--check` takes an *optional* value, so it never reaches for the next
# argument: this checks, and leaves `quiet` an operand that does not exist.
run_msg --check quiet

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

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
# byte-identical output from glibc's sort under the two -- except `-R`. Under
# `C.UTF-8` GNU hashes each key through `strxfrm` with its terminating NUL, and
# under `C` the bare key, so a fixed `--random-source` shuffles differently in
# the two; ours is the UTF-8 locale's, as SlateOS is UTF-8 throughout (§351),
# and `order::random` in the sort crate says how it was found.
#
# What `C.UTF-8` additionally buys is the `argmatch` rows, which used to need a
# reference of their own: gnulib's `quote()` prints U+2018/U+2019 under a UTF-8
# locale and ASCII apostrophes under `C`, and since §351 ours prints the curly
# pair in every locale. Under `C` the *reference* was the wrong one.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one name
# `sort` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG='sort'
# Not the installed binary: WSL's coreutils is Ubuntu's `9.4-3ubuntu6.1` and
# carries behavioural patches, so a green run against it certifies agreement
# with Debian rather than with GNU. See `diff-wsl.sh`'s "Why a built reference"
# and `design-decisions.md` 726.
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Variables the program under test runs with, on both sides, and nothing else
# does. `POSIXLY_CORRECT` changes where option parsing stops, and exported it
# would reach this harness's own `od`, `sort` and `diff` as well. Empty unless
# a case block sets it.
ENVV=()

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
    env ${ENVV[@]+"${ENVV[@]}"} $OURS_RUN "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    env ${ENVV[@]+"${ENVV[@]}"} $GNU_RUN  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
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

run_case()  { compare - "$@"; report "${ENVV[*]:+${ENVV[*]} }sort $*"; }
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

# Every ordering but the default one is handed a *copy* of the key, with what
# -d/-i ignore dropped and the rest translated by -f (upstream's keycompare).
# So -f makes `1m` a mebi-number for -h -- `m` is no unit, `M` is -- and -d
# takes the punctuation out of a version before -V reads it.
printf '1m\n2K\n1k\n3g\n1M\n900\n'                      > humancase.txt
printf 'v1.10\nV1.9\nv1-2\n1.10a\nv1_9\n'               > vcase.txt
run_case -h humancase.txt
run_case -hf humancase.txt
run_case -k1,1hf humancase.txt
run_case -Vf vcase.txt
run_case -Vd vcase.txt
run_case -Vi control.txt
run_case -Vdf vcase.txt
run_case -gf general.txt
run_case -Mf months.txt
run_case -nf spellings.txt

# -g reads with strtold, into an 80-bit long double: a key it cannot read at
# all sorts before everything, the NaNs next (-nan before nan), then the
# numbers -- and two numbers a double cannot tell apart are still two. Its
# leading white space is isspace's, vertical tab and form feed included.
printf 'abc\n0\n-1\nnan\n\n x\n-nan\ninf\n-inf\n'        > gwords.txt
printf '9223372036854775809\n9223372036854775808\n1.0000000000000000009\n1\n' > gprec.txt
printf '\v5\n4\n\f3\n\r2\n'                             > gspace.txt
run_case -g gwords.txt
run_case -gr gwords.txt
run_case -gu gwords.txt
run_case -gs gwords.txt
run_case -k1,1g gwords.txt
run_case -g gprec.txt
run_case -gs gprec.txt
run_case -gsr gprec.txt
run_case -g gspace.txt
# -h knows ronna and quetta, the SI prefixes of 2022.
printf '1Q\n1Y\n1R\n2Z\n-1Q\n1k\n-1R\n'                   > units.txt
run_case -h units.txt
run_case -hr units.txt
# Under -z a record may hold a newline, and the newline is a blank: it
# separates fields, -b skips it, and -d keeps it.
printf 'x\n1\0y 2\0z\n\n0\0'                            > znl.txt
run_case -z -k2,2n znl.txt
run_case -z -k2,2 znl.txt
run_case -z -k2 znl.txt
run_case -z -k2b,2 znl.txt
run_stdin '\nb\0a\0' -zb
run_stdin 'a\nc\0a\nb\0ab\0' -zd
run_stdin ' \n2\0 1\0' -zn

# --- -R: shuffled by a salted MD5 of the key ---------------------------------
# With a fixed --random-source the "random" order is a function of the salt
# (the source's first sixteen bytes) and the keys, so it compares byte for
# byte. Equal keys hash equally, which is what keeps them together.
head -c 16 /dev/zero                                    > zero.src
printf '\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020' > count.src
printf 'abc'                                            > short.src
seq 1 30                                                > thirty.txt
printf 'b\na\nc\nb\na\nd\nA\nB\n a\n'                   > shuffle.txt
run_case -R --random-source=zero.src shuffle.txt
run_case -R --random-source=count.src shuffle.txt
run_case -R --random-source=count.src thirty.txt
run_case --random-sort --random-source=count.src thirty.txt
run_case --sort=random --random-source=count.src thirty.txt
run_case -Rr --random-source=count.src thirty.txt
run_case -Ru --random-source=count.src shuffle.txt
run_case -Rs --random-source=count.src shuffle.txt
run_case -Rf --random-source=count.src shuffle.txt
run_case -Rfu --random-source=count.src shuffle.txt
run_case -Rb --random-source=count.src shuffle.txt
run_case -Rd --random-source=count.src punct.txt
run_case -Ri --random-source=count.src control.txt
run_case -RV --random-source=count.src versions.txt
run_case -VR --random-source=count.src versions.txt
run_case -k2,2R --random-source=count.src cols.txt
run_case -k2,2R -k1,1 --random-source=count.src ties.txt
run_case -k1,1 -k2,2R --random-source=count.src cols.txt
run_case -k2bR,2 --random-source=count.src cols.txt
run_case -R -k2,2 --random-source=count.src cols.txt
run_case -R -k2,2n --random-source=count.src cols.txt
run_case -t: -k2,2R --random-source=count.src colons.txt
run_case -c -R --random-source=count.src thirty.txt
run_case -C -R --random-source=count.src thirty.txt
run_case -m -R --random-source=count.src merge1.txt merge2.txt
run_case -z -R --random-source=count.src nul.txt
run_case -R --random-source=count.src bytes.txt
run_case -R --random-source=count.src empty.txt
# A longer source: only its first sixteen bytes are the salt.
run_case -R --random-source=thirty.txt shuffle.txt
# The source is opened only when a key will be compared at random.
run_case --random-source=nosuchfile cols.txt
run_case -R -k1,1n --random-source=nosuchfile cols.txt
run_case -R --random-source=nosuchfile cols.txt
run_case -R --random-source=short.src cols.txt
run_case -R --random-source=. cols.txt
run_case -R --random-source=count.src --random-source=count.src cols.txt
run_case -R --random-source=count.src --random-source=zero.src cols.txt
run_case -R --random-source=count.src --random-source=./count.src cols.txt
run_case -nR cols.txt
run_case -k1MR cols.txt
run_case -k1RV --random-source=count.src cols.txt
# Without a source the order cannot be known, but a run of equal lines reads
# the same in any order, and the status and the silence are there to compare.
run_stdin 'a\na\na\n' -R
run_stdin '' -R
run_stdin 'x\nx\n' -Ru

# --- --debug: the notes and the underlines -------------------------------------
# Here the stderr *text* is compared as well as stdout and the status: the
# notes are half of what --debug is for. Its stdout is every output line with
# its tabs drawn as `>`, then an underline per key and one for the whole line
# (unless -s or -u), each in columns -- so wide and invalid bytes are cases.
run_debug() {
  local stdin=$1; shift
  local o_err g_err o_bin g_bin o_rc g_rc label
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    $OURS_RUN --debug "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $GNU_RUN  --debug "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
    label="sort --debug $*"
  else
    printf '%b' "$stdin" | $OURS_RUN --debug "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $GNU_RUN  --debug "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
    label="printf '$stdin' | sort --debug $*"
  fi
  if [ "$o_rc" = "$g_rc" ] && cmp -s "$o_bin" "$g_bin" && cmp -s "$o_err" "$g_err"; then
    AGREED=yes
  else
    AGREED=no
    REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
      "$o_rc" "$(od -An -c <"$o_bin" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$o_err")" \
      "$g_rc" "$(od -An -c <"$g_bin" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$g_err")")
  fi
  rm -f "$o_err" "$g_err" "$o_bin" "$g_bin"
  report "$label"
}
run_debug - cols.txt
run_debug - -k2,2 cols.txt
run_debug - -k2,2n cols.txt
run_debug - -k2,1 cols.txt
run_debug - -k3,3 cols.txt
run_debug - -u -k1,1 cols.txt
run_debug - -s -k2b,2 cols.txt
run_debug - -n cols.txt
run_debug - -r cols.txt
run_debug - -r -k1,1 cols.txt
run_debug - -r -k1,1n cols.txt
run_debug - -r -s -k1,1n cols.txt
run_debug - -r -u -k1,1n cols.txt
run_debug - -b cols.txt
run_debug - -k1b,1 cols.txt
run_debug - -k1.2b cols.txt
run_debug - -k1.2,1.3 cols.txt
run_debug - -k1,1.2 cols.txt
run_debug - -k1,1 -k2,2n cols.txt
run_debug - -t ' ' -k2,2n cols.txt
run_debug - -t . -k1n cols.txt
run_debug - -t . -k2,2n cols.txt
run_debug - -t - -k1n cols.txt
run_debug - -t + -k1g cols.txt
run_debug - -t + -k1n cols.txt
run_debug - -t : -k2,2n colons.txt
run_debug - -M months.txt
run_debug - -k1,1M months.txt
run_debug - -h human.txt
run_debug - -g general.txt
run_debug - -k1g,1 general.txt
run_debug - -V versions.txt
run_debug - -R --random-source=count.src cols.txt
run_debug - -d -k1,1n cols.txt
run_debug - -d -f -k1,1n cols.txt
run_debug - -i -b -k1,1 cols.txt
run_debug - -n -k1,1 cols.txt
run_debug - +1 -2 cols.txt
run_debug - +0 cols.txt
run_debug - +1.2 -3.4 cols.txt
run_debug - +1 cols.txt
run_debug - -z nul.txt
run_debug - bytes.txt
run_debug - -u dupes.txt
run_debug - -m merge1.txt merge2.txt
run_debug - empty.txt
run_debug - -c cols.txt
run_debug - -C cols.txt
run_debug - --check=quiet cols.txt
run_debug - -o debug.out cols.txt
run_debug - -c -o debug.out cols.txt
run_debug 'a\tb\n\tx\n'
run_debug 'a\tb\n\tx\n' -k2,2
run_debug '\303\251 z\n\346\227\245\346\234\254 y\n' -k2,2
run_debug '1.5K\n-2\nx\n1.\n.5\n-\n2k\n3m\n' -h
run_debug '1.\n.5\n-.5\n-\nx1\n 1\n12a\n' -n
run_debug '1e3\n0x10\nnan\n  -inf\n\v5\nx\n' -g
run_debug 'jan 1\nxyz\n FEB\nJu\n' -M
run_debug 'a b\n' -k2,2 -k1,1 -k3,3

# --- -z ---------------------------------------------------------------------
run_case -z nul.txt
run_case -zu nul.txt
run_case -z empty.txt
run_stdin 'b\x00a\x00' -z

# --- -c and -C --------------------------------------------------------------
run_case -c sorted.txt
run_case -c unsorted.txt
# The disorder line names the file as given -- bare, and as bytes.
cp unsorted.txt 'un sorted'
cp unsorted.txt "$(printf 'un\377sorted')"
run_case -c 'un sorted'
run_case -c "$(printf 'un\377sorted')"
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

# -R's failures, worded: an unopenable, a short and an unreadable source, two
# different sources, and the orderings it cannot be combined with.
run_msg -R --random-source=nosuchfile
run_msg -R --random-source=short.src
run_msg -R --random-source=.
run_msg -R --random-source=count.src --random-source=zero.src
run_msg --random-source=count.src --random-source=zero.src
run_msg -nR
run_msg -k1MR
run_msg -k1,1Rh
run_msg --sort=random -g

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
# The -k refusals, compared by their words and not only by their status.
# `run_case` further up sees just that stderr is non-empty, and these
# printed ASCII apostrophes -- where upstream's `badfieldspec` and
# `parse_field_count` call quote(), curly under this file's C.UTF-8 -- for
# as long as they existed, unseen by every row that ran them.
run_msg -k0
run_msg -k1.0
run_msg -kx
run_msg -k1x
run_msg -k1,0
run_msg -k1,x
run_msg -k1.
run_msg -k1n,2M
# Which combinations upstream refuses, and through which key. Global options
# are checked as the whole-line key they become, or through the first key that
# inherits them; a key naming an ordering of its own (`d`, even `r`) inherits
# nothing and so is never checked against them. Version and -d/-i share one
# slot, so the -Vd pair is accepted; `f` is listed but never counted.
run_msg -nM
run_msg -n -d
run_msg -gi
run_msg -k1nd
run_msg -fin
run_msg -dMn
run_msg -Vn
run_msg --numeric-sort --month-sort
run_msg --sort=month -n
run_msg -nM -k1
run_msg -nd -k1,1 -k2,2M
run_msg -nM -k2 -k1n
run_msg -k1nM -k0
run_stdin 'b\na\n' -k1Vd
run_stdin 'b\na\n' -k1nf
run_stdin 'b\na\n' -Vi
run_stdin 'b\na\n' -nM -k1,1r
run_stdin 'b\na\n' -nM -k1d
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
# The list is named in the empty-name refusal as upstream's `quotef` names it:
# bare unless it needs quoting, which a space does.
printf 'sorted.txt\0\0' > 'names0 spaced'
run_msg '--files0-from=names0 spaced'
# A value may be written either way round.
run_stdin 'b 1\na 2\n' --key 2
run_stdin 'b 1\na 2\n' --key=2
run_stdin '10\n9\n' --sort numeric
run_stdin '10\n9\n' --sort=numeric
run_stdin 'a\nb\n' --field-separator , -k1
# `--check` takes an *optional* value, so it never reaches for the next
# argument: this checks, and leaves `quiet` an operand that does not exist.
run_msg --check quiet

# --- an input that cannot be read, an output that cannot be written ----------
# stdout, the stderr *text* and the status, under one redirection each. Every
# row here was measured against GNU sort 9.4 on 2026-10-03 and failed before
# that day's conversion:
#
# * Sorting `fstat`s every `-` once its first input is open -- `stat failed:
#   -: Bad file descriptor` -- but a file opened while descriptor 0 is closed
#   becomes descriptor 0, which `xfclose` never closes, so in `sort f -` the
#   `-` is `f` again, at its end. `-c` and `-m` read without the `fstat`.
# * A failure while the lines go out is `write failed`, one at the final
#   `fflush` is `fflush failed`, each naming the output, and `close_stdout`
#   then adds a reason-less `write error`. Which of the two depends on where
#   glibc's buffer flushes, so a small and a large output are both asked.
run_fd() {
  local mode=$1; shift
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$(mktemp); g_out=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)
  # A spelling per mode: a redirection held in a variable would arrive as an
  # argument.
  case $mode in
    stdin-closed)
      $OURS_RUN "$@" <&- >"$o_out" 2>"$o_err"; o_rc=$?
      $GNU_RUN  "$@" <&- >"$g_out" 2>"$g_err"; g_rc=$? ;;
    stdin-dir)
      $OURS_RUN "$@" <. >"$o_out" 2>"$o_err"; o_rc=$?
      $GNU_RUN  "$@" <. >"$g_out" 2>"$g_err"; g_rc=$? ;;
    stdout-closed)
      $OURS_RUN "$@" </dev/null >&- 2>"$o_err"; o_rc=$?
      $GNU_RUN  "$@" </dev/null >&- 2>"$g_err"; g_rc=$? ;;
    stdout-full)
      $OURS_RUN "$@" </dev/null >/dev/full 2>"$o_err"; o_rc=$?
      $GNU_RUN  "$@" </dev/null >/dev/full 2>"$g_err"; g_rc=$? ;;
  esac
  if [ "$o_rc" = "$g_rc" ] && cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err"; then
    AGREED=yes
  else
    AGREED=no
    REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
      "$o_rc" "$(tr '\n' '|' <"$o_out")" "$(tr '\n' '|' <"$o_err")" \
      "$g_rc" "$(tr '\n' '|' <"$g_out")" "$(tr '\n' '|' <"$g_err")")
  fi
  rm -f "$o_out" "$g_out" "$o_err" "$g_err"
  report "sort $* [$mode]"
}
seq 1 100000 > big.txt
run_fd stdin-closed
run_fd stdin-closed -
run_fd stdin-closed -u
run_fd stdin-closed sorted.txt -
run_fd stdin-closed - sorted.txt
run_fd stdin-closed unsorted.txt - sorted.txt
run_fd stdin-closed -c
run_fd stdin-closed -C
run_fd stdin-closed -m -
run_fd stdin-closed -m merge1.txt -
run_fd stdin-closed -m - merge1.txt
run_fd stdin-closed --files0-from=-
run_fd stdin-dir
run_fd stdin-dir -c
run_fd stdin-dir -m -
run_fd stdin-dir unsorted.txt -
run_fd stdout-closed unsorted.txt
run_fd stdout-closed big.txt
run_fd stdout-closed -u unsorted.txt
run_fd stdout-full unsorted.txt
run_fd stdout-full big.txt
run_fd stdout-full -m big.txt big.txt
run_fd stdout-full -o /dev/full unsorted.txt
run_fd stdout-full -o /dev/full big.txt

# --- POSIXLY_CORRECT -----------------------------------------------------------
# glibc's getopt ends option parsing at the first operand while it is set -- to
# anything, the empty string included -- so an option after an operand is an
# operand, and so is a `--` after one, there being no options left for it to
# end. Measured against GNU on 2026-09-25; `coreutils::getopt`'s module docs,
# "Where option parsing stops".
# `sort`'s option string leads with `-`, so getopt itself never stops; sort
# applies the rule by hand, with an exception for a traditional `-o FILE`
# that `_POSIX2_VERSION` in the 2001 window withdraws, as it withdraws `+POS`.
printf 'b\na\n' > posix.txt
ENVV=(POSIXLY_CORRECT=1)
run_case posix.txt -r
run_case -r posix.txt
run_case posix.txt -- -r
run_case posix.txt +1
run_case posix.txt -o posix.out
run_case posix.txt -oposix.out
run_case posix.txt -o
run_case -c posix.txt -o posix.out
ENVV=(POSIXLY_CORRECT=1 _POSIX2_VERSION=200112)
run_case posix.txt -o posix.out
run_case +1 -2 posix.txt
ENVV=(_POSIX2_VERSION=200112)
run_case +1 posix.txt
run_case +1 -2 posix.txt
ENVV=()
run_case posix.txt -r
run_case +1 posix.txt

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

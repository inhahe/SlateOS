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
# `LC_ALL=C` is pinned, and it is not a convenience. In any other locale GNU
# collates with the locale's tables, and SlateOS has none — see
# `known-issues.md`. Comparing against a collating GNU would be comparing
# against a specification we have deliberately not implemented yet.
set -u

# Our sort is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path — `-t/` above all.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/sort.exe"}
GNU=${GNU:-sort}
export LC_ALL=C

if ! printf 'b\na\n' | "$GNU" >/dev/null 2>&1; then
  echo "sort-diff: GNU sort not reachable (tried: $GNU) -- skipping."
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$PWD/$OURS" ;; esac
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
    "$OURS_ABS" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    "$GNU" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | "$OURS_ABS" "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | "$GNU" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
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

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

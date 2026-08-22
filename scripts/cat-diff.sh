#!/usr/bin/env bash
# Differential test: our cat against the host's GNU cat.
#
# Each case is `run_case ARGS...`, run against a fixture directory this script
# builds, with stdout and the exit status compared byte for byte. stdout is
# compared as a hex dump rather than as a shell variable, because `$(...)`
# strips trailing newlines and eats NUL bytes — and the whole claim `cat` makes
# is that it reproduces its input *exactly*, trailing newlines and NULs
# included. Comparing through a variable would discard precisely the evidence.
#
# `run_stdin INPUT ARGS...` is the same over standard input, which is the only
# way to reach the no-trailing-newline and undecodable-byte cases without
# leaving fixture files behind.
#
# stderr only has to agree about *whether* there was a diagnostic. GNU's exact
# wording comes from its getopt and from the host's error table; ours comes
# from `coreutils::errmsg`, which is deliberately POSIX's rather than the
# host's — see the head of `errmsg.rs`.
set -u

# Our cat is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

# Built here, from the package named, rather than picked up out of `target/`.
# A harness that only *runs* that path measures whatever was written there
# last, which need not be current and need not even be this crate — see
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils cat "${OURS:-}") || exit 1
GNU=${GNU:-cat}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

printf 'alpha\tbeta\ngamma\n\n\n\ndelta\n'      > plain.txt
printf 'one\n\n'                                > ends-blank.txt
printf '\n\ntwo\n'                              > starts-blank.txt
printf 'no trailing newline'                    > unterminated.txt
printf 'A\x01\x1f\x7f\x80\xff\xc3\xa9Z\n'       > bytes.txt
printf 'crlf\r\nlines\r\n'                      > crlf.txt
printf 'a\n \n \nb\n'                           > spaces.txt
: > empty.txt

compare() {
  local o_out g_out o_err g_err o_bin g_bin o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file rather than through a pipe into `od`, so that the
  # status we record is cat's own. In `x=$(cat | od)` the exit status belongs to
  # `od`, and `PIPESTATUS` is set inside the command substitution's subshell
  # where it cannot be read — a pipeline here would silently compare od's
  # success against od's success and pass every failure case.
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

run_case() { compare - "$@"; report "cat $*"; }
run_stdin() { local input="$1"; shift; compare "$input" "$@"; report "printf '$input' | cat $*"; }

xfail_case() {
  local reason="$1"; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL cat %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS cat %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# --- the plain copy ---------------------------------------------------------
run_case plain.txt
run_case empty.txt
run_case bytes.txt
run_case crlf.txt
run_case unterminated.txt
run_case plain.txt plain.txt
run_case empty.txt plain.txt empty.txt
run_case -u plain.txt
run_case -- plain.txt

# The claim `cat` makes is byte-for-byte identity, so the cases that can break
# it are the ones a line-splitting implementation gets wrong: no terminator on
# the last line, a CR before the LF, and a byte that is not valid UTF-8.
run_stdin 'x\ny' -
run_stdin 'x\ny'
run_stdin '\x00\x01\xff'
run_stdin 'a\r\nb\r\n'
run_stdin ''

# --- numbering --------------------------------------------------------------
run_case -n plain.txt
run_case -b plain.txt
run_case -n unterminated.txt
run_case -b unterminated.txt
run_case -n empty.txt
run_case -n plain.txt plain.txt
run_case -b plain.txt plain.txt
run_case -n spaces.txt
run_case -b spaces.txt
run_case --number plain.txt
run_case --number-nonblank plain.txt
# `-b` wins over `-n` whichever way round they are written.
run_case -nb plain.txt
run_case -bn plain.txt
run_case -n -b plain.txt
run_case -b -n plain.txt
run_stdin 'a\r\nb\r\n' -n
run_stdin '\xff\xfe\n' -n

# --- squeezing --------------------------------------------------------------
run_case -s plain.txt
run_case -s spaces.txt
run_case -s starts-blank.txt
run_case -s ends-blank.txt
run_case -s empty.txt
# One stream, not two: the run that straddles the join collapses.
run_case -s ends-blank.txt starts-blank.txt
run_case -ns plain.txt
run_case -bs plain.txt
run_case --squeeze-blank plain.txt
run_stdin '\n\n\n' -s
run_stdin '\n' -s

# --- the line-ending and tab markers ----------------------------------------
run_case -E plain.txt
run_case -T plain.txt
run_case -E unterminated.txt
run_case -T unterminated.txt
run_case -E empty.txt
run_case -ET plain.txt
run_case -nE plain.txt
run_case -sE plain.txt
run_case --show-ends plain.txt
run_case --show-tabs plain.txt
run_stdin 'a\tb' -T
run_stdin 'a' -E

# --- the visible rendering of a byte ----------------------------------------
run_case -v bytes.txt
run_case -v plain.txt
run_case -A bytes.txt
run_case -e bytes.txt
run_case -t bytes.txt
run_case -vT bytes.txt
run_case -vE bytes.txt
run_case -nvET bytes.txt
run_case --show-nonprinting bytes.txt
run_case --show-all bytes.txt
run_stdin '\x00\x01\x1f' -v
run_stdin '\x7f\x80\xff' -v
run_stdin '\xc3\xa9' -v
run_stdin 'a\tb' -v
run_stdin 'a\tb' -A
run_stdin '\x00\x80\xff\t' -A

# --- everything at once -----------------------------------------------------
run_case -bsAn plain.txt bytes.txt spaces.txt
run_case -nsET plain.txt starts-blank.txt ends-blank.txt

# --- failure, and the exit status that reports it ---------------------------
# Every one of these exited 0 before the rewrite.
run_case nosuchfile.txt
run_case nosuchfile.txt plain.txt
run_case plain.txt nosuchfile.txt
run_case -n nosuchfile.txt
run_case -Z plain.txt
run_case --nope plain.txt
run_case -nZ plain.txt
run_case -- -Z

# --- option diagnostics, compared word for word against glibc ---------------
#
# The cases above only check *whether* there was a diagnostic, which is right
# for the ones whose text comes from the host's error table. These check the
# text, and so need a reference whose getopt is glibc's.
#
# `$GNU` is not that reference. On this host it is MSYS2's coreutils, and MSYS2
# is a Cygwin derivative: it links `msys-2.0.dll` rather than glibc, and its
# getopt is not glibc's. The two disagree on every sentence here --
# `unknown option -- x` against `invalid option -- 'x'` -- so a harness pointed
# at it certifies wording that no GNU/Linux system prints. `sort-diff.sh` did
# exactly that for eight cases, and passed the whole time. See
# `known-issues.md` -> TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE.
#
# So this section reaches for glibc through WSL, and skips if it is not there
# rather than quietly falling back to a reference that would pass wrongly.
GLIBC=${GLIBC:-"wsl -e env LC_ALL=C cat"}
if printf 'x\n' | $GLIBC >/dev/null 2>&1; then
  HAVE_GLIBC=yes
else
  HAVE_GLIBC=no
  echo "cat-diff: glibc cat not reachable (tried: $GLIBC); skipping option diagnostics"
fi

# Compare stdout, stderr and status exactly, against glibc rather than $GNU.
run_getopt() {
  [ "$HAVE_GLIBC" = yes ] || return 0
  local o_err g_err o_out g_out o_rc g_rc label="cat $*"
  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$("$OURS_ABS" "$@" </dev/null 2>"$o_err"); o_rc=$?
  g_out=$($GLIBC "$@" </dev/null 2>"$g_err"); g_rc=$?
  local o_msg g_msg
  o_msg=$(tr '\n' '|' <"$o_err"); g_msg=$(tr '\n' '|' <"$g_err")
  rm -f "$o_err" "$g_err"
  if [ "$o_msg" = "$g_msg" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s [stderr]\n  ours (rc=%s): %s\n  gnu  (rc=%s): %s\n' \
      "$label" "$o_rc" "$o_msg" "$g_rc" "$g_msg"
  fi
  return 0
}

# The status is 1 here, not sort's 2 -- it is per-utility, and 1 is the common
# case. `cat --zzz-bogus; echo $?` is how to check a newly converted utility.
run_getopt -x
run_getopt -Z
run_getopt --nope
run_getopt --nope=1

# Abbreviation. Every one of these was refused before `cat` used the shared
# getopt, which is the bug that motivated the module.
run_getopt --squeeze
run_getopt --show-a
run_getopt --number-non
run_getopt --num
run_getopt --show
run_getopt --sq=1
run_getopt --show-e=1
run_getopt --number=1

# `--help` and `--version` go through the table like any other option, so they
# refuse an argument the same way rather than printing what was asked for.
run_getopt --help=x
run_getopt --version=x

# The empty prefix matches every option, so this one case pins the table's
# whole declaration order -- which is observable, and which was measured with
# precisely this command rather than recalled.
run_getopt --=x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

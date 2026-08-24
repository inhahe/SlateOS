#!/usr/bin/env bash
# Differential test: our cat against GNU coreutils' cat.
#
# Each case is `run_case ARGS...`, run against a fixture directory this script
# builds, with stdout, stderr and the exit status compared byte for byte.
# stdout is compared as a hex dump rather than as a shell variable, because
# `$(...)` strips trailing newlines and eats NUL bytes — and the whole claim
# `cat` makes is that it reproduces its input *exactly*, trailing newlines and
# NULs included. Comparing through a variable would discard precisely the
# evidence.
#
# `run_stdin INPUT ARGS...` is the same over standard input, which is the only
# way to reach the no-trailing-newline and undecodable-byte cases without
# leaving fixture files behind.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the general reasons. The particular one that
# forced this harness across — it used to build a *native Windows* binary and
# compare it against MSYS2's coreutils — is that `coreutils::stdfd`, the module
# that makes `cat >&-` behave as GNU's does, is `#[cfg(target_os = "linux")]`.
# A Windows build cannot exercise a line of it, so the closed-descriptor cases
# at the foot of this file could not have been written at all, let alone caught
# a regression.
#
# The move also removed a workaround. The option-diagnostics section used to
# need a *second*, WSL-hosted reference of its own, because MSYS2's getopt is
# not glibc's and disagrees with it on every sentence. Both sides are glibc
# now, so those are ordinary cases — and stderr is compared as text throughout
# rather than merely for presence, which is what caught the one real difference
# this migration turned up.
#
# Run `OURS=/usr/bin/cat ./scripts/cat-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `cat` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=cat
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

printf 'alpha\tbeta\ngamma\n\n\n\ndelta\n'      > plain.txt
printf 'one\n\n'                                > ends-blank.txt
printf '\n\ntwo\n'                              > starts-blank.txt
printf 'no trailing newline'                    > unterminated.txt
printf 'A\x01\x1f\x7f\x80\xff\xc3\xa9Z\n'       > bytes.txt
printf 'crlf\r\nlines\r\n'                      > crlf.txt
printf 'a\n \n \nb\n'                           > spaces.txt
: > empty.txt

# One invocation of one side. `$1` is `ours` or `gnu`.
run_side() {
  local side=$1 stdin=$2 out=$3 err=$4; shift 4
  if [ "$stdin" = "-" ]; then
    env PATH="$bindir/$side" cat "$@" </dev/null >"$out" 2>"$err"
  else
    printf '%b' "$stdin" | env PATH="$bindir/$side" cat "$@" >"$out" 2>"$err"
  fi
}

compare() {
  local o_out g_out o_err g_err o_bin g_bin o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file rather than through a pipe into `od`, so that the
  # status we record is cat's own. In `x=$(cat | od)` the exit status belongs to
  # `od`, and `PIPESTATUS` is set inside the command substitution's subshell
  # where it cannot be read — a pipeline here would silently compare od's
  # success against od's success and pass every failure case.
  run_side ours "$stdin" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$stdin" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr is compared as text, not merely for presence. That is only sound
  # because the reference is glibc's: our `errmsg` deliberately prints POSIX's
  # strerror strings rather than the host's, which agrees with glibc and did
  # *not* agree with the Windows host this harness used to run on.
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

echo "cat-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

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

# A directory is not `ENOENT`, and the message says so. It also keeps going:
# the operand after it is still copied.
run_case . plain.txt

# --- option diagnostics, word for word --------------------------------------
#
# These used to need a second, WSL-hosted reference of their own, because the
# reference here was MSYS2's getopt rather than glibc's. Both sides are glibc
# now, so they are ordinary cases — and `compare` checks their stdout as a hex
# dump, which the old bespoke runner did not.
#
# The status is 1 here, not sort's 2 -- it is per-utility, and 1 is the common
# case. `cat --zzz-bogus; echo $?` is how to check a newly converted utility.
run_case -x
run_case -Z
run_case --nope
run_case --nope=1

# Abbreviation. Every one of these was refused before `cat` used the shared
# getopt, which is the bug that motivated the module.
run_case --squeeze
run_case --show-a
run_case --number-non
run_case --num
run_case --show
run_case --sq=1
run_case --show-e=1
run_case --number=1

# `--help` and `--version` go through the table like any other option, so they
# refuse an argument the same way rather than printing what was asked for.
run_case --help=x
run_case --version=x

# The empty prefix matches every option, so this one case pins the table's
# whole declaration order -- which is observable, and which was measured with
# precisely this command rather than recalled.
run_case --=x

# --- what --help and --version print ----------------------------------------
# Recorded as differing on purpose, as in every harness here: our help omits
# the GNU project's `Report bugs to:` block and our version names SlateOS.
xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# --- a standard output that cannot be written -------------------------------
#
# Two destinations, and the difference between them is the point.
#
# `/dev/full` fails every write, and upstream reports it from the `close_stdout`
# atexit hook as `cat: write error: …`. The failure ends the *run*, not just the
# current file: `cat a nope b > /dev/full` prints one write error and never
# mentions `nope`, because the copy of `a` already failed and upstream returns.
#
# A *closed* standard output (`>&-`) is worse, because the Rust runtime hides
# it: it reopens the descriptor on /dev/null before `main` and then maps `EBADF`
# to a completed write, so a program written against `io::stdout()` reports
# success. Upstream never reaches its write path at all here — `cat.c` opens
# with `fstat (STDOUT_FILENO, …)` and dies `cat: standard output: Bad file
# descriptor` before the first operand is opened. That position is observable
# and is asserted below: `cat missing plain.txt >&-` names the descriptor and
# never mentions `missing`.
#
# `--help` and `--version` are the exception, because they print and return
# before that guard: they fail through stdio and are reported by `close_stdout`
# with the other wording. Both are measured here.

writefail() {
  local mode=$1; shift
  local o_err g_err o_rc g_rc side err rc
  o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then err=$o_err; else err=$g_err; fi
    if [ "$mode" = closed ]; then
      env PATH="$bindir/$side" cat "$@" </dev/null >&- 2>"$err"
    else
      env PATH="$bindir/$side" cat "$@" </dev/null >/dev/full 2>"$err"
    fi
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"
  if [ "$o_msg" = "$g_msg" ] && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s}\n  gnu  (rc=%s): err{%s}' \
    "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

writefail closed plain.txt;              report 'cat plain.txt >&-'
writefail closed;                        report 'cat >&-'
writefail closed missing plain.txt;      report 'cat missing plain.txt >&-'
writefail closed plain.txt bytes.txt;    report 'cat plain.txt bytes.txt >&-'
writefail closed -n plain.txt;           report 'cat -n plain.txt >&-'
writefail closed empty.txt;              report 'cat empty.txt >&-'
writefail closed --help;                 report 'cat --help >&-'
writefail closed --version;              report 'cat --version >&-'
# A rejected command line never reaches standard output at all, so closing it
# changes nothing: the diagnostic is the same and so is the status.
writefail closed --bogus;                report 'cat --bogus >&-'

if [ -w /dev/full ]; then
  writefail full plain.txt;              report 'cat plain.txt > /dev/full'
  writefail full plain.txt bytes.txt;    report 'cat plain.txt bytes.txt > /dev/full'
  # One write error, and nothing about `nope`: the first failure ends the run.
  # The fixtures are far smaller than any output buffer, so this is also the
  # case that pins *when* the pending output is delivered -- an implementation
  # that held the first file until the end would report `nope` first and the
  # write error second. All four paths through the copy are checked, because
  # the plain one and the three option-driven ones are different code.
  writefail full plain.txt nope bytes.txt;    report 'cat plain.txt nope bytes.txt > /dev/full'
  writefail full -n plain.txt nope bytes.txt; report 'cat -n plain.txt nope bytes.txt > /dev/full'
  writefail full -A plain.txt nope bytes.txt; report 'cat -A plain.txt nope bytes.txt > /dev/full'
  writefail full -s plain.txt nope bytes.txt; report 'cat -s plain.txt nope bytes.txt > /dev/full'
  # Neither operand opens, so nothing is ever written and the only diagnostics
  # are the two `ENOENT`s -- no write error at all.
  writefail full nope1 nope2;            report 'cat nope1 nope2 > /dev/full'
  # Nothing to copy, so nothing to fail on: silence, and status 0.
  writefail full empty.txt;              report 'cat empty.txt > /dev/full'
  writefail full --help;                 report 'cat --help > /dev/full'
  writefail full --version;              report 'cat --version > /dev/full'
else
  echo "note: no writable /dev/full; the write-error cases did not run" >&2
fi

printf '\ncat: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

#!/usr/bin/env bash
# Differential test: our more against util-linux's more.
#
# Each case is `run_case ARGS...`, run against a fixture directory this script
# builds, with stdout, stderr and the exit status compared byte for byte.
# stdout is compared as a hex dump rather than as a shell variable, because
# `$(...)` strips trailing newlines and eats NUL bytes — and a pager's whole
# claim is that it reproduces its input, trailing newlines and NULs included.
# Comparing through a variable would discard precisely the evidence: the bug
# that prompted this harness was a `more` that *dropped* the last line's
# missing newline and stopped at the first undecodable byte.
#
# ## Why this harness exists
#
# It did not, and four independent truncations lived in a two-hundred-line
# program because of that: a stray non-UTF-8 byte ended the display and exited
# 0; output to anything but a terminal was cut off after one screenful and
# exited 0; a non-UTF-8 file *name* aborted with 134; and a file with no final
# newline gained one. None of them is visible by eye in the source, and every
# one of them is a single line of a hex dump here. See known-issues.md ->
# B-more-STOPPED-PAGING-AT-THE-FIRST-NON-UTF8-BYTE.
#
# ## Why the reference is util-linux and not GNU coreutils
#
# There is no GNU coreutils `more`; `/usr/bin/more` on a Debian-family system
# is util-linux's, and that is what everyone actually runs. Its version is
# printed below so a disagreement can be attributed.
#
# ## What is deliberately not compared
#
# The interactive screen. When stdout *is* a terminal, util-linux drives it
# through terminfo — `\033[7m--More--(Next file: x)\033[27m\r\033[K` — and ours
# writes a plain `--More--`. Those can never match byte for byte, and making
# them match would mean porting terminfo into a pager, which is not what this
# harness is for. What *is* checked here is every cell where the answer must be
# identical: stdout not a terminal (so no paging at all), at both a redirected
# and a terminal stdin. The decision to pause, and the keystroke handling, are
# covered by the unit tests in `more.rs`.
#
# Run `OURS=/usr/bin/more ./scripts/more-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find util-linux's, and put both behind the
# one name `more` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG='more'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

printf 'a\nb\nc\n'                      > plain.txt
printf 'one\ntw\377o\nthree\n'          > bad.txt
printf 'no trailing newline'            > unterminated.txt
printf '\0\1\2\n\377\376\n'             > bytes.txt
printf 'a\n\n\n\nb\n'                   > blanks.txt
printf 'crlf\r\nlines\r\n'              > crlf.txt
printf 'x\ny\n'                         > "$(printf 'caf\351.txt')"
seq 1 60                                > sixty.txt
: > empty.txt
mkdir -p adir bdir sub
printf 'z\n' > sub/nested.txt
printf 'secret\n' > unreadable.txt; chmod 000 unreadable.txt
NONUTF8=$(printf 'caf\351.txt')

# `LINES` is read by both sides, so it is part of the case rather than the
# environment: a screen height that never triggers paging and one that would
# are different code paths, and both must copy the file whole when stdout is
# not a terminal.
LINES_VAL=1000

# One invocation of one side. `$1` is `ours` or `gnu`.
#
# stdin is `/dev/null` — not a terminal — which is half of the banner rule
# util-linux applies (`operands > 1 || !isatty(0)`). The `tty_case` section
# below covers the other half.
run_side() {
  local side=$1 out=$2 err=$3; shift 3
  diff_run env PATH="$bindir/$side" LINES="$LINES_VAL" more "$@" \
    </dev/null >"$out" 2>"$err"
}

compare() {
  local o_out g_out o_err g_err o_bin g_bin o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file rather than through a pipe into `od`, so that the
  # status recorded is more's own; in `x=$(more | od)` it would be od's, and
  # `PIPESTATUS` is set inside a subshell where it cannot be read.
  run_side ours "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr is compared as text, not merely for presence. That is what caught
  # `more: nosuch.txt: No such file or directory (os error 2)` against
  # util-linux's `more: cannot open nosuch.txt: No such file or directory` —
  # two differences in one line, neither of which shows up in a hex dump of
  # stdout.
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

run_case() { compare "$@"; report "LINES=$LINES_VAL more $*"; }

xfail_case() {
  local reason="$1"; shift
  compare "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL more %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS more %s\n  now agrees with util-linux, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

echo "more-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real ($("$gnu_real" --version 2>&1 | head -1))"

# Every case below runs twice: once at a screen height nothing reaches, and
# once at one every fixture exceeds. With stdout not a terminal the two must
# produce identical output, and the second value is what makes that a claim
# rather than an assumption — it is the value under which our `more` used to
# emit one screenful and stop.
for LINES_VAL in 1000 10; do

  # --- the plain copy -------------------------------------------------------
  # A pager's first duty is to reproduce the file. These are the shapes a
  # line-splitting or text-decoding implementation gets wrong.
  run_case plain.txt
  run_case empty.txt
  run_case bytes.txt
  run_case crlf.txt
  run_case blanks.txt
  run_case unterminated.txt
  run_case bad.txt
  run_case sixty.txt

  # --- names ----------------------------------------------------------------
  # The name is echoed back inside the banner, so a lossy decode here names a
  # file the user does not have.
  run_case sub/nested.txt
  run_case "$NONUTF8"

  # --- the banner -----------------------------------------------------------
  # util-linux prints it when there is more than one operand *or* when stdin is
  # not a terminal, which every case here satisfies. What varies is whether a
  # banner is printed for a file that never opens, and whether anything
  # separates one file from the next.
  run_case plain.txt unterminated.txt
  run_case plain.txt bad.txt "$NONUTF8"
  run_case plain.txt plain.txt
  run_case empty.txt plain.txt empty.txt

  # --- operands that do not open --------------------------------------------
  # The banner must not be printed for these: it would name a file and then
  # show nothing under it. Status stays 0 on both sides, which is util-linux's
  # own choice and not an oversight of ours.
  run_case nosuch.txt
  run_case plain.txt nosuch.txt
  run_case nosuch.txt plain.txt
  run_case nosuch.txt nope.txt
  run_case unreadable.txt
  run_case unreadable.txt plain.txt

  # --- directories ----------------------------------------------------------
  # `open` on a directory succeeds and the *read* fails with EISDIR, so a pager
  # that does not stat first prints a banner promising a file and then a
  # diagnostic. util-linux writes `*** NAME: directory ***` to stdout, in band,
  # where the reader is looking.
  run_case adir
  run_case adir plain.txt
  run_case plain.txt adir
  run_case adir bdir

done
LINES_VAL=1000

# --- stdin a terminal -----------------------------------------------------
# The other half of the banner rule, and the only way to reach it: with stdin
# on a pty and stdout still a file, util-linux prints no banner for a lone
# operand and prints one for two. `script` supplies the pty.
if command -v script >/dev/null 2>&1; then
  # Not `run_side`: `script` takes the command as one string for a shell it
  # starts itself, so the redirections have to be inside that string rather
  # than applied to the call — which is also why the fixture names in these
  # cases are all shell-safe.
  tty_case() {
    local label="$1"; shift
    local o_out g_out o_rc g_rc
    script -qec "env PATH=$bindir/ours LINES=$LINES_VAL more $* > $DIFF_TMP/o.bin 2> $DIFF_TMP/o.err" \
      /dev/null </dev/null >/dev/null 2>&1
    o_rc=$?
    script -qec "env PATH=$bindir/gnu LINES=$LINES_VAL more $* > $DIFF_TMP/g.bin 2> $DIFF_TMP/g.err" \
      /dev/null </dev/null >/dev/null 2>&1
    g_rc=$?
    o_out=$(od -An -tx1 <"$DIFF_TMP/o.bin"); g_out=$(od -An -tx1 <"$DIFF_TMP/g.bin")
    local o_msg g_msg
    o_msg=$(cat "$DIFF_TMP/o.err"); g_msg=$(cat "$DIFF_TMP/g.err")
    rm -f "$DIFF_TMP/o.bin" "$DIFF_TMP/g.bin" "$DIFF_TMP/o.err" "$DIFF_TMP/g.err"
    if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
      AGREED=yes
    else
      AGREED=no
    fi
    REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
      "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
      "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
    report "$label"
  }

  # One operand: no banner on either side. This is the cell that a rule keyed
  # on stdout instead of stdin gets wrong.
  tty_case 'stdin a tty: more plain.txt > file' plain.txt
  # Two: banners on both, and nothing between the files.
  tty_case 'stdin a tty: more plain.txt sixty.txt > file' plain.txt sixty.txt
  # A screen height every fixture exceeds, still with stdout a file: neither
  # side pages, because paging is stdout's business, not stdin's.
  LINES_VAL=10
  tty_case 'stdin a tty, LINES=10: more sixty.txt > file' sixty.txt
  LINES_VAL=1000
else
  echo "note: no 'script' inside WSL; the terminal-stdin cases did not run" >&2
fi

# --- a full disk ----------------------------------------------------------
# util-linux writes no diagnostic and exits 0 when its output cannot be
# written, which is why `more.rs` discards write errors on purpose. If that
# ever stops being true, this is where it shows.
if [ -w /dev/full ]; then
  full_case() {
    local label="$1"; shift
    local o_err g_err o_rc g_rc o_msg g_msg
    o_err=$(mktemp); g_err=$(mktemp)
    diff_run env PATH="$bindir/ours" LINES=$LINES_VAL more "$@" \
      </dev/null >/dev/full 2>"$o_err"; o_rc=$?
    diff_run env PATH="$bindir/gnu" LINES=$LINES_VAL more "$@" \
      </dev/null >/dev/full 2>"$g_err"; g_rc=$?
    o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err"); rm -f "$o_err" "$g_err"
    if [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then AGREED=yes; else AGREED=no; fi
    REPORT=$(printf '  ours (rc=%s): {%s}\n  gnu  (rc=%s): {%s}' \
      "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
      "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')")
    report "$label"
  }
  full_case 'more plain.txt > /dev/full' plain.txt
  full_case 'more plain.txt nosuch.txt > /dev/full' plain.txt nosuch.txt
else
  echo "note: no writable /dev/full; the write-error cases did not run" >&2
fi

# --- the one deliberate divergence ----------------------------------------
xfail_case \
  "util-linux has no '-' convention and reports 'cannot open -'; every other utility in this tree spells stdin '-', and a pager that alone refused it would be the surprise. known-issues.md -> B-more-STOPPED-PAGING-AT-THE-FIRST-NON-UTF8-BYTE" \
  -

chmod 644 unreadable.txt 2>/dev/null

printf '\nmore: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with the reference is never worse — but
# it does mean a recorded decision has gone stale, so it must not pass
# silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

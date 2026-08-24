#!/usr/bin/env bash
# Differential test: our `yes` against GNU coreutils'.
#
# ## Bounding a program that never stops
#
# `yes` has no end, so every case that actually runs it is bounded by piping
# stdout through `head -c 40000` and comparing those bytes. The limit is
# deliberately several times the 8 KiB buffer: the whole point of buffering is
# that many records go out in one `write`, and the failure it could introduce —
# a record split across a buffer boundary, or a buffer that is not a whole
# number of records — is invisible in the first 4 KiB and obvious in 40 KB.
#
# For those cases the program's own exit status is *not* compared, and the
# reason is a real difference rather than a limitation of the method:
#
#   GNU `yes` dies of `SIGPIPE` when `head` goes away, and a shell reports 141.
#   SlateOS has no Unix signals for process control (`design.txt`) and Rust
#   masks the signal anyway, so the same situation arrives as `EPIPE`, is
#   recognised, and exits 0 quietly. `cut`, `head`, `tail` and `uniq` in this
#   tree already do exactly that.
#
# That difference is not swept under the bound — it is one explicit `xfail`
# below, so it is recorded once instead of being smeared silently over every
# case. Cases that *do* terminate on their own — `--help`, `--version`, every
# option error — compare the status like any other harness.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the general reasons. Two are particular to this
# program: the fixtures include an argument that is not valid UTF-16 and so
# cannot exist on Windows at all, and `yes >&-` needs `coreutils::stdfd`, which
# is `#[cfg(target_os = "linux")]` — without it the program does not merely
# report the wrong thing, it never terminates.
#
# ## Cases that differ on purpose
#
# Two: `--help` omits the GNU project's `Report bugs to:` block and `--version`
# names SlateOS, as everywhere here. Plus the broken-pipe status above.
#
# Run `OURS=/usr/bin/yes ./scripts/yes-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `yes` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=yes
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# Several times the 8 KiB output buffer, so a record split across a write shows.
LIMIT=40000

# `ENDLESS` marks a case that never terminates: stdout is bounded by `head` and
# the program's own status is not compared. See the header.
ENDLESS=

# `KIND` marks a case that asks *which* of the three things the program did, not
# what it said doing it. `--h`, `--he` and `yes a --help` exist to show that an
# abbreviation resolves, and that an option keeps its meaning after an operand;
# they are not a second copy of the `--help` test. Comparing the full text there
# would fail on the one difference already recorded as an xfail — our help drops
# the GNU bug-report block and our version names SlateOS — and would fail for
# every case, which is how a known difference stops being a record and becomes
# noise. So stdout is reduced to its class and the classes are compared:
#
#   help    stdout begins `Usage: yes `
#   version stdout begins `yes (`
#   empty   nothing on stdout, i.e. the parser rejected the arguments
#   stream  anything else: the repeated record
#
# stderr and the exit status are still compared byte for byte, so a case that
# resolves to the right class by the wrong route is still caught. And the
# classes are distinct enough to catch the mistakes these cases are for: an
# abbreviation resolving to the wrong option is help-vs-version, and one
# wrongly rejected as ambiguous is help-vs-empty plus a diagnostic.
KIND=

reset_knobs() { ENDLESS=; KIND=; }

# The class of one output file, per the table above.
classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: yes }" != "$first" ]; then echo help
  elif [ "${first#yes \(}" != "$first" ]; then echo version
  else echo stream
  fi
}

# --- run one case on both sides ----------------------------------------------
compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$(mktemp); g_out=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  local side out err rc rcf
  rcf=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_out; err=$o_err
    else out=$g_out; err=$g_err; fi
    # `head -c` is what bounds the run; `timeout` is the backstop for a version
    # that manages to produce nothing at all, which would otherwise hang here
    # forever rather than failing.
    #
    # The status wanted is the program's, not `head`'s, and it is carried out of
    # the pipeline in a file rather than in `${PIPESTATUS[0]}`. `PIPESTATUS` is
    # a bashism, and `scripts/all-diff.sh` runs every harness as `sh "$h"` —
    # under a `sh` that is not bash it would silently read as empty and every
    # status comparison here would compare nothing against nothing. The `echo`
    # writes to the file, not to the closed pipe, so it survives the `SIGPIPE`
    # that ends the endless cases.
    { timeout -k 2 60 env PATH="$bindir/$side" yes "$@" 2>"$err"; echo $? >"$rcf"; } \
      | head -c "$LIMIT" >"$out"
    rc=$(cat "$rcf")
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done
  rm -f "$rcf"

  local o_sum g_sum o_msg g_msg
  if [ -n "$KIND" ]; then
    o_sum="class $(classify "$o_out")"
    g_sum="class $(classify "$g_out")"
  else
    # A digest rather than the bytes: 40 KB of `y\n` in a report helps nobody,
    # and the length is carried alongside so a truncation is still legible.
    o_sum="$(wc -c <"$o_out") $(cksum <"$o_out")"
    g_sum="$(wc -c <"$g_out") $(cksum <"$g_out")"
  fi
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  # The first line as text, so a wrong *string* is readable in the report
  # without dumping the whole bound.
  local o_first g_first
  o_first=$(head -c 120 "$o_out" | head -1)
  g_first=$(head -c 120 "$g_out" | head -1)
  rm -f "$o_out" "$g_out" "$o_err" "$g_err"

  local status_ok=yes
  [ -z "$ENDLESS" ] && [ "$o_rc" != "$g_rc" ] && status_ok=no

  if [ "$o_sum" = "$g_sum" ] && [ "$o_msg" = "$g_msg" ] && [ "$status_ok" = yes ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s first{%s} err{%s}\n  gnu  (rc=%s): %s first{%s} err{%s}' \
    "$o_rc" "$o_sum" "$o_first" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$g_sum" "$g_first" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  reset_knobs
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report "yes $*"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS yes %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail yes %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

echo "yes-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. The repeated line
# =============================================================================

ENDLESS=1; run_case
ENDLESS=1; run_case no
ENDLESS=1; run_case a b c
ENDLESS=1; run_case ''                       # one empty operand: a blank line
ENDLESS=1; run_case a '' b                   # the separator still comes
ENDLESS=1; run_case 'hello world' x
ENDLESS=1; run_case κόσμε
ENDLESS=1; run_case a - b                    # a lone `-` is an operand
ENDLESS=1; run_case ' '                      # a single space
ENDLESS=1; run_case 'trailing '              # trailing blank belongs to it

# A record longer than the buffer, so the doubling has nothing to do and the
# write path is exercised at a size it cannot amortise.
ENDLESS=1; run_case "$(head -c 20000 /dev/zero | tr '\0' 'x')"

# Odd record lengths, where a buffer rounded to a byte count rather than
# doubled would split a record and produce a stream neither side agrees with.
ENDLESS=1; run_case abc
ENDLESS=1; run_case abcde
ENDLESS=1; run_case abcdefg

# The argument that started this: bytes that are not valid UTF-8. The previous
# version panicked here before printing anything.
ENDLESS=1; run_case "$(printf 'na\377me')"
ENDLESS=1; run_case "$(printf '\377')" "$(printf '\200\201')"

# =============================================================================
# 2. `--` and things that look like options
# =============================================================================

ENDLESS=1; run_case -- --help
ENDLESS=1; run_case -- -x
ENDLESS=1; run_case --                       # no operands left: back to `y`
ENDLESS=1; run_case -- --
ENDLESS=1; run_case -
# Endless, not an error: after `--` there are no options left to reject, so
# `-x` is the first word of a record rather than an unknown option.
ENDLESS=1; run_case -- -x --help

# =============================================================================
# 3. Option errors — these terminate, so the status is compared too
# =============================================================================

run_case -x
run_case -xy
run_case --nope
run_case --=x                 # names every long option, in table order
run_case --help=x             # takes no argument
run_case --version=x

# =============================================================================
# 4. Options are recognised after operands
# =============================================================================
# glibc permutes and upstream does not pass `+`, so an option keeps its meaning
# wherever it appears. `yes a --help` prints the help. What is asserted is that
# it reaches help at all, so these compare the class — see `KIND` above.

KIND=1; run_case a --help
run_case a -x                 # an error terminates, so this one compares fully
KIND=1; run_case '' --version

# =============================================================================
# 5. Abbreviations
# =============================================================================
# Same again: the question is which option an abbreviation resolves to, not what
# that option prints.

KIND=1; run_case --h
KIND=1; run_case --he
KIND=1; run_case --v
KIND=1; run_case --ver

# =============================================================================
# 6. --help and --version
# =============================================================================

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 7. A write that fails for a reason other than a dead reader
# =============================================================================
# Not run through `head`: these have to reach the real error path, and they
# terminate on their own.
#
# Two destinations, and the difference between them is the point. `/dev/full`
# fails every write; a *closed* standard output (`>&-`) is worse, because Rust
# hides it — the runtime reopens the descriptor on /dev/null before `main` and
# then maps `EBADF` to a completed write, so `yes >&-` through `io::stdout()`
# does not merely report the wrong thing, it never ends. The `timeout` below is
# what turns that into a failed case rather than a hung harness.
#
# The two diagnostics are also worded differently, and that is upstream's doing
# rather than an accident: the endless loop is `full_write` on the descriptor
# and reports `standard output: …` itself, while `--help` and `--version` print
# through stdio and are reported by the `close_stdout` atexit hook as
# `write error: …`. Both are measured here.

writefail() {
  local mode=$1; shift
  local o_err g_err o_rc g_rc side err rc
  o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then err=$o_err; else err=$g_err; fi
    if [ "$mode" = closed ]; then
      timeout -k 2 60 env PATH="$bindir/$side" yes "$@" >&- 2>"$err"
    else
      timeout -k 2 60 env PATH="$bindir/$side" yes "$@" >/dev/full 2>"$err"
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
    "$o_rc" "$o_msg" "$g_rc" "$g_msg")
}

if [ -w /dev/full ]; then
  writefail full; report 'yes > /dev/full'
  writefail full y; report 'yes y > /dev/full'
else
  echo "note: no writable /dev/full; the write-error case did not run" >&2
fi

writefail closed;             report 'yes >&-'
writefail closed y;           report 'yes y >&-'
writefail closed --help;      report 'yes --help >&-'
writefail closed --version;   report 'yes --version >&-'
# A rejected command line never reaches standard output at all, so closing it
# changes nothing: the diagnostic is the same and so is the status.
writefail closed --bogus;     report 'yes --bogus >&-'

# =============================================================================
# 8. The broken-pipe status, recorded once
# =============================================================================
# Every ENDLESS case above compared content only, for this reason. Here it is
# on its own so the difference is a line in the summary rather than a silence.

pipestatus() {
  local side rc o_rc g_rc rcf
  rcf=$(mktemp)
  for side in ours gnu; do
    { env PATH="$bindir/$side" yes 2>/dev/null; echo $? >"$rcf"; } \
      | head -c 10 >/dev/null
    rc=$(cat "$rcf")
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done
  rm -f "$rcf"
  if [ "$o_rc" = "$g_rc" ]; then AGREED=yes; else AGREED=no; fi
  REPORT=$(printf '  ours rc=%s\n  gnu  rc=%s' "$o_rc" "$g_rc")
}

pipestatus
if [ "$AGREED" = yes ]; then
  xpass=$((xpass+1))
  printf 'XPASS yes | head  (expected to differ: SIGPIPE kills GNU, we exit 0)\n'
else
  xfail=$((xfail+1))
  [ -n "${VERBOSE:-}" ] && printf 'xfail yes | head  (SIGPIPE kills GNU, we exit 0)\n'
fi

# The wording is the family's, not this harness's own: `scripts/all-diff.sh`
# decides green by matching " 0 differed" in the tail line, so a summary that
# said "0 failed" would be reported as a failing harness forever.
printf '\nyes: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0

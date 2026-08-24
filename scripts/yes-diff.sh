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
# The same reasons as `cmp-diff.sh`, `tee-diff.sh` and `digest-diff.sh`, whose
# headers spell them out: the fixtures include an argument that is not valid
# UTF-16 and so cannot exist on Windows at all, and a Linux build sharing the
# repository's `target/` with the Windows one would make each invalidate the
# other. The build lands in `$HOME/.cache/slateos-diff-target` inside WSL,
# shared with the other harnesses (`design-decisions.md` §374).
#
# ## Cases that differ on purpose
#
# Two: `--help` omits the GNU project's `Report bugs to:` block and `--version`
# names SlateOS, as everywhere here. Plus the broken-pipe status above.
#
# Run `OURS=/usr/bin/yes ./scripts/yes-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# --- get ourselves into WSL --------------------------------------------------
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "yes-diff: no WSL on this host; skipping (ours is a unix-only binary)"
    exit 0
  fi
  here=$(cd "$(dirname "$0")" && pwd)
  if command -v cygpath >/dev/null 2>&1; then here=$(cygpath -m "$here"); fi
  inside=$(wsl wslpath -u "$here" 2>/dev/null) || {
    echo "yes-diff: could not map $here into WSL; skipping"
    exit 0
  }
  exec wsl -e env "OURS=${OURS:-}" "VERBOSE=${VERBOSE:-}" \
    bash "$inside/yes-diff.sh"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- the reference -----------------------------------------------------------
if ! command -v yes >/dev/null 2>&1; then
  echo "yes-diff: no GNU yes inside WSL; skipping"
  exit 0
fi

# --- the subject -------------------------------------------------------------
OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "yes-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  target_dir=$HOME/.cache/slateos-diff-target
  ( cd "$root" && cargo build -p coreutils --bin yes \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  OURS=$target_dir/x86_64-unknown-linux-gnu/debug/yes
fi
if [ ! -x "$OURS" ]; then
  echo "yes-diff: $OURS is not executable" >&2
  exit 1
fi
case $OURS in
  /*) ;;
  *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
esac

# Diagnostics are referenced under a UTF-8 locale, as everywhere since §351:
# getopt renders an unknown or ambiguous option with directional single quotes
# under a UTF-8 locale and ASCII apostrophes under `C`, so the whole
# option-error family would disagree for a reason unrelated to this program.
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

# --- one name for both sides -------------------------------------------------
# Each binary is reached through a symlink called `yes`, in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word
# `yes` on both sides and the `yes: ` prefix on every diagnostic matches.
gnu_real=$(command -v yes)
bindir=$(mktemp -d)
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/yes"
ln -s "$gnu_real" "$bindir/gnu/yes"
trap 'rm -rf "$bindir"' EXIT

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
# Not run through `head`: this one has to reach the real error path, and it
# terminates on its own.

writefail() {
  local o_err g_err o_rc g_rc side err rc
  o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then err=$o_err; else err=$g_err; fi
    timeout -k 2 60 env PATH="$bindir/$side" yes >/dev/full 2>"$err"
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
  writefail; report 'yes > /dev/full'
else
  echo "note: no writable /dev/full; the write-error case did not run" >&2
fi

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

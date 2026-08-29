#!/usr/bin/env bash
# Differential test: our xargs against GNU xargs (findutils 4.9.0).
#
# ## The thing this harness has to solve that the others do not
#
# Every other `*-diff.sh` compares two programs that *write* something. `xargs`
# writes almost nothing: it builds an argument vector and hands it to a command,
# and the argument vector is the entire product. Comparing "what appeared on
# stdout" therefore compares the wrong program — whatever the command chose to
# print — and loses precisely the distinctions this conversion exists to
# preserve. `echo` cannot tell you whether `a b` arrived as one argument or two;
# it cannot show you an empty argument at all; and it renders a byte that is not
# UTF-8 identically to one that is.
#
# So the command both sides run is a fixture of this harness, `argv`, and it
# prints the vector rather than interpreting it:
#
#     RUN argc=3\n  arg\0 arg\0 arg\0
#
# One line naming the invocation — which is how a case sees *how many times* the
# command ran, the observable that `-n`, `-L`, `-s` and `--max-procs` all exist
# to control — and then every argument raw and NUL-terminated. The harness
# compares the whole stream with `od -An -c`, so an empty argument, an embedded
# newline, a lone `\351` and a run that happened twice instead of once are all
# distinguishable, and none of them survives a comparison of `echo`'s output.
#
# ## Why both sides run inside WSL, and behind one name
#
# `scripts/diff-wsl.sh` gives the general reasons: the reference has to be
# glibc's, and ours has to be a Linux binary. Two of its mechanisms are
# load-bearing here in particular:
#
#   * **`argv[0]` is the bare word `xargs` on both sides.** Every diagnostic
#     xargs prints is prefixed with it, and the `-t` trace is nothing but a
#     rendering of a command line, so a harness that reached one side by an
#     absolute path would differ on every erroring case for a reason that has
#     nothing to do with the program.
#
#   * **`PATH` is a single directory.** That is not merely tidiness — it decides
#     one of the exit statuses under test. `xargs.c:1360` exits `127` when
#     `execvp` sets `ENOENT` and `126` otherwise, and glibc's `execvp`, having
#     searched a `PATH` and found nothing executable, reports the *most
#     specific* failure it saw: any `EACCES` on the way beats the `ENOENT` at
#     the end. This machine's ambient `PATH` holds three Windows-interop
#     directories that are not searchable, so `xargs nosuchcmd` exits **126**
#     there and **127** here. Both are correct; the one here is the one that is
#     a property of `xargs`. See `known-issues.md` →
#     `B-XARGS-IS-A-STUB-AND-DASH-ZERO-CANNOT-CARRY-THE-BYTES-IT-EXISTS-FOR`,
#     which records the other one, since this harness cannot.
#
# The fixture commands live in *both* `$bindir/ours` and `$bindir/gnu`, for the
# same reason: `xargs` resolves the command word on `PATH`, so the two sides
# must each have their own copy under the same name, or one of them fails to
# find it.
#
# ## What is deliberately not tested
#
# * **`-P` above 1.** Two children writing to one pipe interleave, and the order
#   is the kernel's to choose. Parallelism is covered by its option parsing, by
#   `-P 1` (where the ordering is total), and by `--process-slot-var` under
#   `-P 1`, where the slot is always 0.
#
# * **`-o` (`--open-tty`) with no controlling terminal.** GNU 4.9.0 aborts:
#   the child's `/dev/tty` open fails, it dies through `die()` — which is
#   `exit()`, not `_exit()` — and so runs the parent's `atexit` hook, whose
#   first act is `assert (getpid () == parent)`. The result is
#   `xargs.c:1605: wait_for_proc_all: Assertion 'getpid () == parent' failed`
#   and `terminated by signal 6`. That is an upstream bug, not a behaviour to
#   transcribe, so `-o` is tested for its parsing and not for that path.
#
# * **`-p` (`--interactive`) with a terminal.** It would block on a human. The
#   cases run it under `setsid`, where there is no controlling terminal and the
#   `/dev/tty` open fails deterministically — which still exercises that `-p`
#   implies `-t`, and in what order the trace and the diagnostic appear.
#
#     sh scripts/xargs-diff.sh                       # run it
#     OURS=/usr/bin/xargs sh scripts/xargs-diff.sh   # control: should be all green
#
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one name
# `xargs` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=xargs
DIFF_NEED="od setsid"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# Absolute, because the cases run with `PATH` set to one directory that does not
# contain it.
SETSID=$(command -v setsid)

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"

# --- the fixture commands, in both sides' PATH directories --------------------
#
# `argv` is the whole point; the rest are the smallest set of commands the cases
# need xargs to run. Each is written once and linked into both directories, so
# that neither side can be running a different fixture from the other.
helpers=$DIFF_TMP/helpers
mkdir -p "$helpers"

# Print this invocation's argument vector, raw. See the header.
#
# `od` is not used here: the arguments are written as bytes and the *harness*
# dumps the resulting stream, so this stays one process per invocation rather
# than one per argument — which matters for the cases that build thousands.
cat >"$helpers/argv" <<'EOF'
#!/bin/sh
printf 'RUN argc=%d\n' "$#"
for a in "$@"; do printf '%s\0' "$a"; done
exit 0
EOF

# The same, plus the process-slot variable, for `--process-slot-var`.
cat >"$helpers/slot" <<'EOF'
#!/bin/sh
printf 'RUN argc=%d slot=%s\n' "$#" "${XSLOT-unset}"
for a in "$@"; do printf '%s\0' "$a"; done
exit 0
EOF

chmod +x "$helpers/argv" "$helpers/slot"

# A regular file that is *found* on PATH and cannot be executed: the unambiguous
# 126, the one that does not depend on how the search went.
printf 'not a program\n' >"$helpers/notexec"
chmod 644 "$helpers/notexec"

# Two more names for the same two directories, of the *same length*.
#
# `--show-limits` and the `-s` too-large warning print a number derived from
# `bc_size_of_environment ()`, the total bytes of `environ`. Each side is run as
# `env PATH="$bindir/$side" xargs …`, and `ours` is four bytes where `gnu` is
# three — so the two sides' environments differed by exactly one byte and every
# such number differed by one. That is not a difference between the programs; it
# is a difference between the two ways this harness starts them, and it showed
# up on the control run where both sides were the same binary.
#
# Equal-length aliases fix it without touching `diff-wsl.sh`, whose `ours`/`gnu`
# naming is right for the other 48 harnesses and is what makes a failure report
# legible. Command lookup follows the symlink, so each alias reaches the same
# `xargs` and the same fixtures as the directory it points at.
ln -s "$bindir/ours" "$bindir/pathA"
ln -s "$bindir/gnu"  "$bindir/pathB"

side_dir() {
  case $1 in
    ours) printf '%s/pathA' "$bindir" ;;
    *)    printf '%s/pathB' "$bindir" ;;
  esac
}

for side in ours gnu; do
  for h in argv slot notexec; do
    ln -s "$helpers/$h" "$bindir/$side/$h"
  done
  # Borrowed from the system: the cases need a shell to produce a chosen exit
  # status or signal, and `true`/`false` for the statuses that have no output.
  for h in sh true false; do
    for cand in "/bin/$h" "/usr/bin/$h"; do
      [ -x "$cand" ] && { ln -s "$cand" "$bindir/$side/$h"; break; }
    done
  done
done

# --- input fixtures for `-a` --------------------------------------------------
cd "$fixtures" >/dev/null || exit 1
printf 'one two\nthree\n'        >plain.txt
printf 'a\0b\0c\0'               >nul.txt
printf 'caf\351 na\357ve\n'      >latin1.txt
: >empty.txt

# --- one invocation of one side ----------------------------------------------
# Each side is reached through a symlink named `xargs` in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word on
# both sides. `diff_run` keeps bash's own announcement of a child that died of a
# signal out of the stderr the caller captures; `diff-wsl.sh` says why.
run_side() { local side=$1; shift; diff_run env PATH="$(side_dir "$side")" xargs "$@"; }

# The same, with no controlling terminal, so that `/dev/tty` cannot be opened.
run_side_tty() {
  local side=$1; shift
  diff_run env PATH="$(side_dir "$side")" "$SETSID" -w xargs "$@"
}

# `compare STDIN RUNNER ARGS...`
#
# STDIN is `-` for `/dev/null`, or a string fed through `printf '%b'`. Octal
# escapes in it must be written `\0NNN`, which is the form `%b` defines; a bare
# `\NNN` is not portable across the two printfs this file can end up using.
compare() {
  local stdin=$1 runner=$2; shift 2
  local o_out g_out o_err g_err o_rc g_rc o_bin g_bin o_msg g_msg
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(xargs | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. Same note as cat-diff.sh and tr-diff.sh.
  o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    $runner ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $runner gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | $runner ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $runner gnu  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  # `-v`: without it `od` folds repeated lines to `*`, which would hide a
  # difference in *how many* identical arguments were built.
  o_out=$(od -An -c -v <"$o_bin"); g_out=$(od -An -c -v <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: xargs's diagnostics
  # are half of what it does — the `-t` trace, the mutually-exclusive warnings,
  # `--show-limits` and the whole usage message all go there — so a harness that
  # only asked "did it complain?" would pass on every wording this exists to fix.
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  # Truncated: a splitting case builds thousands of arguments, and an
  # untruncated dump of one would bury every other line of the run.
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-500)" \
    "$(printf '%s' "$o_msg" | tr '\n' '|' | cut -c1-500)" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-500)" \
    "$(printf '%s' "$g_msg" | tr '\n' '|' | cut -c1-500)")
  rm -f "$o_err" "$g_err"
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

# A case with input: the first argument is fed to `printf '%b'` and piped in.
run_in() {
  local input="$1"; shift
  compare "$input" run_side "$@"
  report "printf '$input' | xargs $*"
}

# A case with no input at all — for the diagnostics decided before a byte is
# read, and for `-a`, which ignores stdin.
run_case() { compare - run_side "$@"; report "xargs $*"; }

# A case run without a controlling terminal, for `-p`.
run_tty() {
  local input="$1"; shift
  compare "$input" run_side_tty "$@"
  report "printf '$input' | setsid xargs $*"
}

xfail_case() {
  local reason="$1"; shift
  compare - run_side "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL xargs %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS xargs %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# Two of our own invocations compared against each other. The reference cannot
# arbitrate an abbreviation whose long form is *meant* to differ from GNU's, but
# the abbreviation must still resolve to the same option.
selfsame() {
  local a="$1" b="$2" x y xr yr
  # shellcheck disable=SC2086  # both are single options by construction
  x=$(env PATH="$(side_dir ours)" xargs $a </dev/null 2>&1); xr=$?
  y=$(env PATH="$(side_dir ours)" xargs $b </dev/null 2>&1); yr=$?
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   xargs %s == xargs %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF xargs %s != xargs %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- the default command ------------------------------------------------------
# With no command at all xargs runs `/bin/echo`, which is the one case where the
# command is not a fixture of this harness. It is still the same binary on both
# sides, so the comparison holds.
run_in 'a b c\n'
run_in 'a b c\n' -n 2
run_in '\n'
run_in ''

# --- plain splitting ----------------------------------------------------------
run_in 'a b c\n' argv
run_in 'a\nb\nc\n' argv
run_in 'a\tb\tc\n' argv
run_in 'a  b\t\tc\n' argv
run_in '   a b   \n' argv
run_in 'a b c' argv
run_in '\n\n\na\n\n\n' argv
run_in 'a' argv
run_in ' ' argv
run_in '\t\n' argv
# Empty input still runs the command once, unless -r says otherwise. This is the
# rule our stub got backwards.
run_in '' argv
run_in '' -r argv
run_in '   \n' argv
run_in '   \n' -r argv
run_in '' --no-run-if-empty argv

# --- POSIX's blank is not C's space -------------------------------------------
# `read_line` starts in the SPACE state and skips `ISSPACE`, but once in NORM
# only `ISBLANK` — space and tab — ends an argument. So a form feed or a
# vertical tab is stripped from the front of the input and is an ordinary byte
# in the middle of it. Upstream comments the distinction explicitly; it is not
# an accident, and it is the kind of thing a from-scratch implementation gets
# wrong by reaching for `split_whitespace`.
run_in 'a\013b\n' argv
run_in 'a\014b\n' argv
run_in '\013a b\n' argv
run_in '\014a b\n' argv
run_in '\013\014 a\n' argv
run_in 'a \013 b\n' argv
run_in 'a\rb\n' argv
run_in '\ra\n' argv

# --- quoting ------------------------------------------------------------------
run_in "'a b'\n" argv
run_in '"a b"\n' argv
run_in "a'b c'd\n" argv
run_in 'a"b c"d\n' argv
run_in "'a\"b'\n" argv
run_in '"a\0047b"\n' argv
run_in "''\n" argv
run_in '""\n' argv
run_in "'' ''\n" argv
run_in "x '' y\n" argv
# A quote may span a newline; the newline is then an ordinary byte.
run_in "'a\nb'\n" argv
run_in '"a\nb"\n' argv
run_in "'a\tb'\n" argv
# Backslash escapes the next byte, including a blank, a quote and a newline.
run_in 'a\\ b\n' argv
run_in 'a\\\tb\n' argv
run_in "a\\\\'b\n" argv
run_in 'a\\"b\n' argv
run_in 'a\\\\b\n' argv
run_in 'a\\\nb\n' argv
run_in '\\ a\n' argv
# ... but not inside quotes, where it is an ordinary byte.
run_in "'a\\\\b'\n" argv
run_in '"a\\\\b"\n' argv
run_in "'a\\\\'\n" argv
# Unmatched quotes are fatal, at end of line and at end of file alike.
run_in "'a\n" argv
run_in '"a\n' argv
run_in "'a" argv
run_in '"a' argv
run_in "a 'b c\n" argv
run_in "'\n" argv
# A trailing backslash at EOF.
run_in 'a\\' argv
run_in 'a b\\' argv

# --- the eof string -----------------------------------------------------------
run_in 'a\nSTOP\nb\n' -E STOP argv
run_in 'a\nSTOP\nb\n' --eof=STOP argv
run_in 'a STOP b\n' -E STOP argv
# It has to be the whole argument, not a prefix of one.
run_in 'a\nSTOPX\nb\n' -E STOP argv
run_in 'a\nXSTOP\nb\n' -E STOP argv
run_in 'a\n STOP \nb\n' -E STOP argv
# An empty eof string turns the feature off, which is how you ask for no logical
# EOF at all while still passing an -E.
run_in 'a\n\nb\n' -E '' argv
run_in 'a\nSTOP\nb\n' -E '' argv
# The deprecated -e, whose argument is optional.
run_in 'a\nSTOP\nb\n' -eSTOP argv
run_in 'a\nSTOP\nb\n' -e argv
run_in 'a\n_\nb\n' -e argv
run_in 'a\nSTOP\nb\n' --eof argv
# -0 and -d turn the eof string off entirely.
run_in 'a\0STOP\0b\0' -0 -E STOP argv
run_in 'a,STOP,b,' -d , -E STOP argv
# A quoted eof word is the eof word once the quotes are gone.
run_in "a\n'STOP'\nb\n" -E STOP argv

# --- -0 / --null --------------------------------------------------------------
run_in 'a\0b\0c\0' -0 argv
run_in 'a\0b\0c' -0 argv
run_in '\0' -0 argv
run_in 'a\0\0b\0' -0 argv
run_in '' -0 argv
run_in 'a\0b\0' --null argv
# In -0 mode quotes, backslashes and blanks are all ordinary bytes: that is the
# entire reason the mode exists.
run_in "'a b'\0" -0 argv
run_in 'a\\ b\0' -0 argv
run_in 'a\tb\0' -0 argv
run_in 'a\nb\0' -0 argv
run_in '"unmatched\0' -0 argv
# ... and so are bytes that are not UTF-8. `caf\351` is Latin-1 `café`, the
# canonical filename `find -print0 | xargs -0` exists to carry.
run_in 'caf\0351\0' -0 argv
run_in 'caf\0351\0na\0357ve\0' -0 argv
run_in '\0377\0376\0' -0 argv
run_in '\0200\0' -0 argv
# A lone high byte in an argument xargs merely passes through, not in an
# argument it has to parse.
run_in 'caf\0351\n' argv
run_in 'a\0351b c\n' argv

# --- non-UTF-8 in argv, not merely in the input -------------------------------
# This is the property the conversion away from `String` argv exists to
# establish, and every case above tests the *input* side of it. A byte that is
# not UTF-8 is a legal filename on this OS, so it reaches xargs as an
# INITIAL-ARG, as the -I pattern's surroundings, as -E's logical EOF string and
# as -a's operand — none of which the input cases touch. `\351` is Latin-1 `é`.
e=$(printf 'caf\351')
run_in 'a b\n' argv "$e"
run_in 'a b\n' -n 1 argv "$e"
# Substitution both ways: a non-UTF-8 item into an ASCII arg, and an ASCII item
# into a non-UTF-8 one. The pattern search has to be over bytes for either.
run_in 'caf\0351\n' -I@ argv 'pre@post'
run_in 'x\n' -I@ argv "$(printf 'p\351@s\351')"
run_in 'caf\0351\n' -I@ argv "$(printf '@\351@')"
# The -t trace and the -p prompt render arguments through the shell-escape
# quoting style, which has to decide what to do with a byte it cannot decode.
run_in 'a\n' -t argv "$e"
run_in 'caf\0351 a\n' -t argv
run_in 'caf\0351\n' -t -I@ argv 'pre@post'
# A logical EOF string that is not UTF-8, matched against an item that is not
# either. EOF_STR compares the first byte before it calls strcmp, so a mismatch
# in the high byte alone still has to be a mismatch.
run_in 'caf\0351\na\n' -E "$e" argv
run_in 'caf\0350\na\n' -E "$e" argv
# -a naming a file that does not exist, so the diagnostic has to quote a name
# it cannot decode. This is the case that arbitrates our quoting against GNU's
# for undecodable bytes, which no ASCII case can.
run_case -a "$(printf 'no\351such')" argv
# A raw high byte as the delimiter, rather than -d's own \351 escape. Upstream
# stores it in a signed char, so it can never equal a getc result and the input
# never splits; ours has to reproduce that, not "fix" it.
run_in 'a\0351b\0351' -d "$(printf '\351')" argv
run_in 'a\0351b\0351' -d "$(printf '\377')" argv
# --process-slot-var and the replacement pattern itself, named in bytes.
run_in 'a b\n' --process-slot-var="$e" slot
run_in 'a\n' -I "$(printf '\351')" argv "$(printf 'p\351s')"
unset e

# --- -d / --delimiter ---------------------------------------------------------
run_in 'a,b,c,' -d , argv
run_in 'a,b,c' -d , argv
run_in 'a,,b,' -d , argv
run_in ',' -d , argv
run_in 'a,b,' --delimiter=, argv
run_in 'a b,c d,' -d , argv
run_in "'a',b," -d , argv
run_in 'a\tb\tc\t' -d '\t' argv
run_in 'a\nb\nc\n' -d '\n' argv
run_in 'a\0b\0' -d '\0' argv
run_in 'a\0b\0' -d '\\0' argv
run_in 'aXbXc' -d X argv
run_in 'a\0351b\0351' -d '\0351' argv
# The escape grammar of -d's own argument.
run_case -d '\\t' argv
run_case -d '\\n' argv
run_case -d '\\\\' argv
run_case -d '\\x' argv
run_case -d '' argv
run_case -d ab argv
run_case -d '\\101' argv
run_case -d '\\0101' argv

# --- -n / --max-args ----------------------------------------------------------
run_in 'a b c d e\n' -n 1 argv
run_in 'a b c d e\n' -n 2 argv
run_in 'a b c d e\n' -n 3 argv
run_in 'a b c d e\n' -n 5 argv
run_in 'a b c d e\n' -n 6 argv
run_in 'a b c d e\n' -n 100 argv
run_in 'a\n' -n 1 argv
run_in '' -n 1 argv
run_in '' -n 1 -r argv
run_in 'a b c\n' --max-args=2 argv
# With initial arguments, -n counts only the ones read from input.
run_in 'a b c d\n' -n 2 argv X Y
run_in 'a b c d\n' -n 1 argv X
# Diagnostics.
run_case -n 0 argv
run_case -n -1 argv
run_case -n x argv
run_case -n '' argv
run_case -n 2x argv
run_case -n ' 2' argv
run_case -n 99999999999999999999 argv

# --- -L / -l / --max-lines ----------------------------------------------------
run_in 'a b\nc d\ne f\n' -L 1 argv
run_in 'a b\nc d\ne f\n' -L 2 argv
run_in 'a b\nc d\ne f\n' -L 3 argv
run_in 'a b\nc d\ne f\n' -L 9 argv
run_in 'a b\nc d\n' --max-lines=1 argv
# A line whose last byte before the newline is a blank continues onto the next.
run_in 'a \nb\nc\n' -L 1 argv
run_in 'a\t\nb\nc\n' -L 1 argv
run_in 'a\nb \nc\n' -L 2 argv
run_in 'a \n \nb\n' -L 1 argv
# An empty line is not an argument, and the question is whether it counts as a
# line for -L's purposes.
run_in 'a\n\nb\n' -L 1 argv
run_in 'a\n\n\nb\n' -L 2 argv
# The deprecated -l, whose argument is optional and defaults to 1.
run_in 'a b\nc d\n' -l argv
run_in 'a b\nc d\ne f\n' -l2 argv
run_in 'a b\nc d\n' --max-lines argv
# -L and -n are mutually exclusive and the later one wins, with a warning naming
# the one that lost.
run_in 'a b c d\n' -n 2 -L 1 argv
run_in 'a b c d\n' -L 1 -n 2 argv
run_in 'a b c d\n' -l -n 2 argv
run_in 'a b c d\n' -n 2 -l argv
# Diagnostics.
run_case -L 0 argv
run_case -L -1 argv
run_case -L x argv
run_case -l 0 argv
run_case -L '' argv

# --- -I / -i / --replace ------------------------------------------------------
run_in 'a\nb\n' -I '{}' argv '{}'
run_in 'a\nb\n' -I '{}' argv 'pre{}post'
run_in 'a\nb\n' -I '{}' argv '{}' '{}'
run_in 'a\nb\n' -I '{}' argv 'x' '{}' 'y'
run_in 'a\nb\n' -I@ argv '@'
run_in 'a\nb\n' --replace argv '{}'
run_in 'a\nb\n' --replace='{}' argv '{}'
run_in 'a\nb\n' -i argv '{}'
run_in 'a\nb\n' -i@ argv '@'
# The replacement string not appearing at all: the command runs once per line
# with no substitution.
run_in 'a\nb\n' -I '{}' argv 'x'
# -I takes the whole line, blanks and all, as one argument — quoting still
# applies, but a bare blank does not split.
run_in 'a b\nc d\n' -I '{}' argv '{}'
run_in "'a b'\nc\n" -I '{}' argv '{}'
run_in '  a b  \nc\n' -I '{}' argv '{}'
# An empty line under -I.
run_in 'a\n\nb\n' -I '{}' argv '{}'
# -I implies -r: empty input runs nothing at all.
run_in '' -I '{}' argv '{}'
# The replacement is not applied to arguments read from input, only to the
# initial ones — there are none read after the first under -I.
run_in 'a\n' -I '{}' argv '{}' '{}' '{}'
# Bytes that are not UTF-8, through the replacement.
run_in 'caf\0351\n' -I '{}' argv '{}'
run_in 'a\n' -I '{}' argv 'x\0351{}'
# -I and -n / -L are mutually exclusive, except that `-i -n1` is specifically
# excused upstream (savannah patch #1500).
run_in 'a\nb\n' -I '{}' -n 2 argv '{}'
run_in 'a\nb\n' -n 2 -I '{}' argv '{}'
run_in 'a\nb\n' -I '{}' -n 1 argv '{}'
run_in 'a\nb\n' -n 1 -I '{}' argv '{}'
run_in 'a\nb\n' -I '{}' -L 2 argv '{}'
run_in 'a\nb\n' -L 2 -I '{}' argv '{}'
# Diagnostics.
run_case -I '' argv '{}'
run_case --replace= argv '{}'

# --- -s / --max-chars ---------------------------------------------------------
# Small sizes make the splitting observable without depending on ARG_MAX.
run_in 'aa bb cc dd ee ff\n' -s 30 argv
run_in 'aa bb cc dd ee ff\n' -s 40 argv
run_in 'aa bb cc dd ee ff\n' -s 60 argv
run_in 'aa bb cc dd ee ff\n' -s 100 argv
run_in 'aa bb cc dd ee ff\n' --max-chars=40 argv
# An argument that cannot fit at all: without -x xargs runs it anyway on a line
# of its own; with -x it is fatal.
run_in 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa b\n' -s 25 argv
run_in 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa b\n' -s 25 -x argv
run_in 'a b\n' -s 25 -x argv 'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ'
# -s counts the initial arguments too.
run_in 'aa bb cc dd\n' -s 40 argv 'iiiiiiiiii'
# The size limits themselves.
run_case -s 0 argv
run_case -s -1 argv
run_case -s x argv
run_case -s 1 argv
run_case -s 99999999999 argv

# --- -t / --verbose -----------------------------------------------------------
run_in 'a b\n' -t argv
run_in 'a b c\n' -t -n 2 argv
run_in 'a b\n' --verbose argv
run_in '' -t argv
run_in '' -t -r argv
run_in "'a b'\n" -t argv
run_in 'caf\0351\n' -t argv
run_in 'a\nb\n' -t -I '{}' argv '{}'
run_in 'a b\n' -t false
run_in 'a b\n' -t nosuchcommand_zz

# --- -x / --exit --------------------------------------------------------------
run_in 'a b c\n' -x -n 2 argv
run_in 'a b c\n' --exit -n 2 argv
run_in 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n' -x -s 20 argv

# --- -p / --interactive, without a controlling terminal -----------------------
# `-p` implies `-t`, so the trace is written before the prompt is attempted;
# the order of the two on stderr is part of what is compared.
run_tty 'a b\n' -p argv
run_tty 'a b\n' --interactive argv
run_tty '' -p argv
run_tty 'a b\n' -p -t argv
run_tty 'a b\n' -t -p argv

# --- -P / --max-procs and --process-slot-var ----------------------------------
run_in 'a b c d\n' -P 1 -n 1 argv
run_in 'a b c d\n' --max-procs=1 -n 2 argv
run_in 'a b\n' -P 0 -n 2 argv
run_in 'a b c\n' -P 1 -n 1 --process-slot-var=XSLOT slot
run_in 'a b\n' -P 1 --process-slot-var=XSLOT slot
run_in 'a b\n' --process-slot-var=XSLOT slot
run_case -P x argv
run_case -P -1 argv
run_case -P '' argv
run_case --process-slot-var=A=B argv
run_case --process-slot-var= argv

# --- -a / --arg-file ----------------------------------------------------------
run_case -a plain.txt argv
run_case --arg-file=plain.txt argv
run_case -a nul.txt -0 argv
run_case -a latin1.txt argv
run_case -a empty.txt argv
run_case -a empty.txt -r argv
run_case -a nosuchfile.txt argv
run_case -a . argv
# -a wins over stdin, which is why this case has input and ignores it.
run_in 'ignored\n' -a plain.txt argv

# --- --show-limits ------------------------------------------------------------
# Every number in it is derived from `ARG_MAX`, the stack limit and the size of
# the environment, all three of which are identical for the two sides because
# both are started the same way from this shell.
run_case --show-limits argv
run_case --show-limits -s 100 argv
run_case -S argv
run_in 'a b\n' --show-limits argv

# --- exit statuses ------------------------------------------------------------
run_in 'a\n' true
run_in 'a\n' false
run_in 'a\n' sh -c 'exit 0'
run_in 'a\n' sh -c 'exit 1'
run_in 'a\n' sh -c 'exit 42'
run_in 'a\n' sh -c 'exit 255'
run_in 'a\n' sh -c 'kill -TERM $$'
run_in 'a\n' sh -c 'kill -KILL $$'
run_in 'a\n' nosuchcommand_zz
run_in 'a\n' notexec
run_in 'a\n' /nonexistent/dir/cmd
run_in 'a\n' /etc
# 255 aborts the whole run, so the second line never executes; 1 does not.
run_in 'a\nb\n' -n 1 sh -c 'exit 255'
run_in 'a\nb\n' -n 1 sh -c 'exit 1'
run_in 'a\nb\n' -n 1 -x sh -c 'exit 1'
# The status of the last child does not win; the worst one does.
run_in 'a b\n' -n 1 sh -c 'if [ "$1" = a ]; then exit 1; fi' sh

# --- the `+` in the option string ---------------------------------------------
# `getopt_long` is called with a leading `+`, so option parsing stops at the
# first non-option: everything after the command word belongs to the command,
# even when it looks exactly like one of xargs's own options.
run_in 'a\n' argv -n
run_in 'a\n' argv -t
run_in 'a\n' argv --verbose
run_in 'a\n' argv -0
run_in 'a\n' -t argv -n 2
run_in 'a\n' argv -- -x
# `--` still ends xargs's own options.
run_in 'a\n' -- argv
run_in 'a\n' -t -- argv
run_case --

# --- option diagnostics -------------------------------------------------------
run_case -Z argv
run_case --nosuch argv
run_case --nosuch=1 argv
run_case -n
run_case -s
run_case -d
run_case -I
run_case -a
run_case -E
run_case --max-args
run_case --delimiter
run_case --arg-file
run_case --eof=
# Options that take no argument still refuse one.
run_case --null=x argv
run_case --verbose=x argv
run_case --exit=x argv
run_case --show-limits=x argv
run_case --no-run-if-empty=x argv
run_case --interactive=x argv
run_case --open-tty=x argv
run_case --help=x
run_case --version=x
# Unambiguous abbreviations resolve.
run_in 'a b c\n' --max-a 2 argv
run_in 'a b\n' --verb argv
run_in 'a\0b\0' --nu argv
run_in 'a b\n' --no-run argv
# Ambiguous ones list their candidates in GNU's declaration order. `--v` is one
# of them here, where in most of these utilities it is `--version`: xargs has a
# `--verbose` too, so the abbreviation that other harnesses check with
# `selfsame` is a diagnostic in this one.
run_case --v argv
run_case --m argv
run_case --e argv
run_case --ma argv
run_case --max argv
run_case --n argv
run_case --a argv
run_case --p argv
run_case --s argv
# Clustering, and an option argument attached to its letter.
run_in 'a b c d\n' -tn2 argv
run_in 'a\0b\0' -0t argv
run_in 'a b c\n' -n2 argv
run_in 'a,b,' -d, argv
run_in 'a b\n' -sx argv
run_in 'a b c d\n' -xn2 argv

# --- the command word is not optional in every position -----------------------
run_case
run_case -n 2
run_case -t
run_case -0
run_case -I '{}'

# --- long input, past the buffer ----------------------------------------------
# GNU's line buffer is `arg_max + 1` bytes and ours need not be the same size,
# so agreement across the seam is the whole point. Two thousand arguments also
# means the default `-s` actually has to split, which no small case reaches.
many=$(seq 1 2000 | tr '\n' ' ')
compare "$many\n" run_side argv; report "2000 args | xargs argv"
compare "$many\n" run_side -n 100 argv; report "2000 args | xargs -n 100 argv"
compare "$many\n" run_side -s 200 argv; report "2000 args | xargs -s 200 argv"
compare "$many\n" run_side -L 1 argv; report "2000 args | xargs -L 1 argv"
# One argument longer than any sensible buffer, which must survive whole.
long=$(printf 'a%.0s' $(seq 1 5000))
compare "$long\n" run_side argv; report "one 5000-byte arg | xargs argv"
compare "$long\n" run_side -s 1000 argv; report "one 5000-byte arg | xargs -s 1000 argv"
compare "$long\n" run_side -s 1000 -x argv; report "one 5000-byte arg | xargs -s 1000 -x argv"
# The same with an explicit delimiter, so the quoting machinery is out of the
# picture and only the buffer arithmetic is left.
manydelim=$(seq 1 2000 | tr '\n' ',')
compare "$manydelim" run_side -d , argv; report "2000 comma args | xargs -d , argv"
compare "$manydelim" run_side -d , -s 200 argv; report "2000 comma args | xargs -d , -s 200 argv"

# --- differ on purpose --------------------------------------------------------
# `--help`'s body matches GNU's; what follows it does not, and must not. GNU
# closes it with a referral to the GNU project's bug address and documentation,
# which name an upstream this is not. `--version` likewise names SlateOS rather
# than GNU findutils 4.9.0 with its copyright and authors.
xfail_case help-closes-with-a-referral-to-the-gnu-project-which-this-is-not --help
xfail_case version-names-slateos-not-gnu-findutils --version
# The abbreviations above still have to resolve, which the comparison cannot
# show while the outputs are expected to differ.
selfsame --he --help
selfsame --hel --help
selfsame --vers --version
selfsame --versi --version

# --- summary ------------------------------------------------------------------
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d XPASS' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]

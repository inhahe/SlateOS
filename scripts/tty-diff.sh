#!/bin/sh
# tty-diff.sh — compare our `tty` against the real GNU one, inside WSL.
#
# ## What this is checking
#
# `tty` prints the name of the terminal on **standard input**, and its exit
# status is the answer to "is there one". Almost every property of it is a
# status rather than a string, which is why the old implementation could be
# wrong in five ways and still look right: it printed `not a tty` when there
# was no terminal, and that is what you notice.
#
# The groups here:
#
#   * **the answer, and where it comes from** — upstream calls `ttyname(0)` and
#     nothing else; the comment in `tty.c` says "POSIX requires that ttyname (0)
#     be used here". The old code asked `isatty(0)` first and invented
#     `/dev/tty` when `ttyname` then returned null, so a descriptor that is a
#     terminal with no name got a name that is not its own and exit 0 where GNU
#     says `not a tty` and exits 1. Both sides are run **inside one pty**, by
#     one `script` invocation, so the name is compared byte for byte rather
#     than compared to a pattern — two separate ptys would have two names and
#     the case would have to be weakened to a regex to pass.
#   * **the stdin cases** — a pipe, a file, a directory, `/dev/null`, and `<&-`.
#     The last is the one worth having: a *closed* descriptor 0 is not a
#     terminal, but it fails with `EBADF` rather than `ENOTTY`, and a `tty` that
#     reported the errno instead of the question would differ there.
#   * **`-s` and its two long spellings** — `--silent` and `--quiet`, which the
#     old code did not have at all; `-s` was matched by comparing each argument
#     to the string `-s`, so `tty -- -s` was silent (GNU: an operand) and
#     `tty -sq` was not (GNU: fine, `-s` twice over is still `-s`).
#   * **the operands** — `tty` takes none, and refuses the first extra one with
#     gnulib's *locale* `quote()`, so `‘x’` and not `'x'`. **Exit 2**, not 1:
#     `tty.c` calls `usage (TTY_FAILURE)` with `TTY_FAILURE = 2`, because 1 is
#     already spoken for as "no terminal".
#   * **the write errors** — `>&-` and `>/dev/full`. **Exit 3**, not 1:
#     `initialize_exit_failure (TTY_WRITE_ERROR)` with `TTY_WRITE_ERROR = 3`,
#     and it overrides the earned status, so `tty >&-` with no terminal exits 3
#     and not 1. `tty -s >&-` exits 1, because `-s` wrote nothing and gnulib's
#     `close_stream` forgives an `EBADF` with nothing pending.
#
# ## Cases that differ on purpose
#
# Two kinds, every one recorded as `xfail`: `--help` omits the GNU project's
# ancillary block, as every converted utility here does, and `--version` names
# SlateOS.
#
# Run `OURS=/usr/bin/tty ./scripts/tty-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG=tty
DIFF_NEED=script
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- knobs, reset before every case ------------------------------------------
# `STDIN_FROM` is a path to read standard input from; `STDIN_CLOSED` closes it
# instead, which is a different thing and not a spelling of the same one —
# `/dev/null` is a descriptor that is not a terminal, `<&-` is not a descriptor.
# `TO_FULL` and `CLOSED` are the two ways to reach a write error: /dev/full
# fails every write, whereas a closed descriptor only fails one that was
# actually attempted.
STDIN_FROM=; STDIN_CLOSED=; TO_FULL=; CLOSED=

reset_knobs() { STDIN_FROM=; STDIN_CLOSED=; TO_FULL=; CLOSED=; }

# Bytes, not text: a terminal's name is a path, and a path is bytes.
render() {
  local f=$1 sz
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  od -An -c <"$f"
}

# Each argument single-quoted, for the pty case, which has to hand both sides
# to `script` as one command string.
shq() {
  local a
  for a in "$@"; do
    printf " '%s'" "$(printf '%s' "$a" | sed "s/'/'\\\\''/g")"
  done
}

# The verdict, from four files. Shared by both runners below.
judge() {
  local o_bin=$1 g_bin=$2 o_err=$3 g_err=$4 o_rc=$5 g_rc=$6
  local o_out g_out o_msg g_msg
  o_out=$(render "$o_bin"); g_out=$(render "$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

# --- run one case on both sides, with standard input under our control -------
compare() {
  local o_bin g_bin o_err g_err o_rc g_rc
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  local side out err rc sin
  sin=${STDIN_FROM:-/dev/null}
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_bin; err=$o_err
    else out=$g_bin; err=$g_err; fi
    # Four spellings rather than a variable holding a redirection: the shell
    # expands redirections before it expands variables, so `$REDIR` would be an
    # argument and not a redirection.
    if [ -n "$STDIN_CLOSED" ]; then
      ( timeout -k 2 60 env PATH="$bindir/$side" tty "$@" ) >"$out" 2>"$err" <&-
    elif [ -n "$CLOSED" ]; then
      ( timeout -k 2 60 env PATH="$bindir/$side" tty "$@" ) >&- 2>"$err" <"$sin"
    elif [ -n "$TO_FULL" ]; then
      ( timeout -k 2 60 env PATH="$bindir/$side" tty "$@" ) >/dev/full 2>"$err" <"$sin"
    else
      ( timeout -k 2 60 env PATH="$bindir/$side" tty "$@" ) >"$out" 2>"$err" <"$sin"
    fi
    # On the very next line, before anything else runs — including a `[ ]`
    # test, whose own status would silently replace it. See tee-diff.sh, where
    # getting this wrong cost a full run.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  judge "$o_bin" "$g_bin" "$o_err" "$g_err" "$o_rc" "$g_rc"
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  reset_knobs
}

# --- run one case on both sides, on one shared pty ---------------------------
# Both sides in a single `script`, so `ttyname(0)` has the same answer for each
# and the name can be compared as bytes. Standard *output* still goes to a
# file: `tty` reads descriptor 0, so redirecting 1 away from the pty changes
# nothing it looks at, and it is the only way to capture what it printed.
pty_compare() {
  local o_bin g_bin o_err g_err o_rcf g_rcf o_rc g_rc qa redir
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)
  o_rcf=$(mktemp); g_rcf=$(mktemp)
  qa=$(shq "$@")

  if [ -n "$CLOSED" ]; then redir='>&-'
  elif [ -n "$TO_FULL" ]; then redir='>/dev/full'
  else redir=; fi

  # `script -q -e -c CMD /dev/null`: -q suppresses its banner, -e makes its own
  # status the child's, -c gives the command, and /dev/null discards the
  # typescript. Its stdout is redirected too, or the pty's echo would land in
  # the harness's output.
  script -qec "\
env PATH='$bindir/ours' tty$qa ${redir:->'$o_bin'} 2>'$o_err'; echo \$? >'$o_rcf'; \
env PATH='$bindir/gnu'  tty$qa ${redir:->'$g_bin'} 2>'$g_err'; echo \$? >'$g_rcf'" \
    /dev/null >/dev/null 2>&1

  o_rc=$(cat "$o_rcf"); g_rc=$(cat "$g_rcf")
  judge "$o_bin" "$g_bin" "$o_err" "$g_err" "${o_rc:-<none>}" "${g_rc:-<none>}"
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err" "$o_rcf" "$g_rcf"
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

# The label is built before the runner runs, because the runner resets the
# knobs.
label_of() {
  printf 'tty %s%s%s%s%s' "$*" \
    "${STDIN_FROM:+  [<$STDIN_FROM]}" "${STDIN_CLOSED:+  [<&-]}" \
    "${TO_FULL:+  [>/dev/full]}" "${CLOSED:+  [>&-]}"
}
run_case() { local label; label=$(label_of "$@"); compare "$@"; report "$label"; }
pty_case() { local label; label="[pty] $(label_of "$@")"; pty_compare "$@"; report "$label"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- fixtures ----------------------------------------------------------------
printf 'contents\n' >"$DIFF_TMP/plain"
mkdir -p "$DIFF_TMP/adir"

# --- no terminal on standard input -------------------------------------------
# The ordinary answer, and the one the old code got right, which is why nothing
# else about it was noticed.
run_case
STDIN_FROM=$DIFF_TMP/plain; run_case
STDIN_FROM=$DIFF_TMP/adir; run_case
# A closed descriptor 0 is not a terminal either, but `ttyname` fails with
# EBADF rather than ENOTTY. Same answer, different reason — which is the point:
# `tty` reports the question, not the errno.
STDIN_CLOSED=1; run_case

# --- a real terminal ---------------------------------------------------------
# Both sides on one pty, so the name is compared and not merely its shape.
pty_case
pty_case -s
pty_case --silent
pty_case --quiet

# --- the silent flag, without a terminal -------------------------------------
# Prints nothing either way; the whole content of the case is the status.
run_case -s
run_case --silent
run_case --quiet
# Prefixes: `--s` and `--q` are unambiguous, `silent` and `quiet` being the only
# long options that begin with those letters.
run_case --sil
run_case --qu
run_case --s
run_case --q
# Twice over is still once.
run_case -ss
run_case -s -s
run_case -s --quiet

# --- operands ----------------------------------------------------------------
# There are none. The first extra one is named, and the rest are not — and the
# status is 2, because 1 already means "no terminal".
run_case x
run_case a b
run_case ''
run_case -
# After `--` it is an operand however it is spelled, which is exactly what the
# old string-matching `-s` got wrong.
run_case -- -s
run_case -- x
run_case -- --help

# --- the option errors -------------------------------------------------------
run_case -x
run_case -sx
run_case --nope
run_case ---help
run_case --silent=1
run_case --quiet=
run_case --help=1

# --- help and version --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The operand check happens after the whole scan, so an option still wins over
# an operand that precedes it.
xfail_case 'our --help omits the GNU project ancillary block' x --help
xfail_case 'our --version names SlateOS' --version --help
xfail_case 'our --help omits the GNU project ancillary block' -s --help
# Prefixes: `--h` and `--v` are unambiguous.
xfail_case 'our --help omits the GNU project ancillary block' --h
xfail_case 'our --version names SlateOS' --v

# --- standard output closed, and full ----------------------------------------
# Status 3, and it overrides the 1 the run had earned.
CLOSED=1; run_case
TO_FULL=1; run_case
CLOSED=1; run_case --help
TO_FULL=1; run_case --version
# `-s` writes nothing, so a closed stdout is not a failed write: status stays 1.
CLOSED=1; run_case -s
TO_FULL=1; run_case -s
# The usage error never gets as far as a flush, so a closed stdout adds nothing
# to it: the operand complaint is the whole output, and the status stays 2.
CLOSED=1; run_case x
CLOSED=1; run_case -x
# And on a terminal, where the earned status is 0 rather than 1.
CLOSED=1; pty_case
TO_FULL=1; pty_case
CLOSED=1; pty_case -s

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]

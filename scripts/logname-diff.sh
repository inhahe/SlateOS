#!/bin/sh
# logname-diff.sh — compare our `logname` against the real GNU one, inside WSL.
#
# ## What this is checking
#
# `logname` and `whoami` look like the same utility and are not. `whoami` asks
# the kernel who you *are* (`getpwuid(geteuid())`); `logname` asks the
# user-accounting database who *logged in* (`getlogin(3)`). After `su alice`
# they disagree, which is the entire reason both exist. The old implementation
# read `$LOGNAME` then `$USER` — and since `su` sets `$LOGNAME`, it gave
# `whoami`'s answer under `logname`'s name.
#
# The groups here:
#
#   * **the environment is ignored** — `LOGNAME=zzz USER=zzz logname` must not
#     print `zzz`. In this WSL image nothing writes utmp, so `getlogin` returns
#     null and both sides say `no login name`; that is a real check, because
#     the old code printed `$LOGNAME` in exactly this situation.
#   * **the operands** — `logname` takes none, and refuses the first extra one
#     with gnulib's *locale* `quote()`, so `‘x’` and not `'x'`.
#   * **the option errors** — an empty `getopt_long` string, so `-x` is
#     `invalid option -- 'x'` rather than an operand.
#   * **the write errors** — `>&-` and `>/dev/full`, the sweep this file was
#     written for. Rust reopens a closed descriptor on /dev/null before `main`
#     and then reports writes to a closed one as successes, so both of these
#     were a silent exit 0 before `coreutils::stdfd`.
#
# Note that both sides call the same glibc `getlogin` here, so the harness is
# not asserting *which* answer that gives — only that ours comes from there
# and not from the environment. On SlateOS the answer comes from our own
# `posix::pwd::getlogin`, which is a constant; see the file's module docs.
#
# ## Cases that differ on purpose
#
# Two kinds, every one recorded as `xfail`: `--help` omits the GNU project's
# ancillary block, as every converted utility here does, and `--version` names
# SlateOS.
#
# Run `OURS=/usr/bin/logname ./scripts/logname-diff.sh` to confirm the harness
# still discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG=logname
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- knobs, reset before every case ------------------------------------------
# `ENVV` is a word list handed to `env`, so it can hold both assignments
# (`LOGNAME=zzz`) and `env`'s own flags (`-u LOGNAME`). `TO_FULL` and `CLOSED`
# are the two ways to reach a write error — they are not interchangeable:
# /dev/full fails every write, whereas a closed descriptor only fails one that
# was actually attempted.
ENVV=; TO_FULL=; CLOSED=; ERR_FULL=; ERR_CLOSED=

reset_knobs() { ENVV=; TO_FULL=; CLOSED=; ERR_FULL=; ERR_CLOSED=; }

# Bytes, not text: a login name comes out of the accounting database and the
# comparison must not go through something that would normalise it.
render() {
  local f=$1 sz
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  od -An -c <"$f"
}

# --- run one case on both sides ----------------------------------------------
compare() {
  local o_bin g_bin o_err g_err o_rc g_rc
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  local side out err rc
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_bin; err=$o_err
    else out=$g_bin; err=$g_err; fi
    # `$ENVV` is deliberately unquoted: it is a word list.
    # A spelling per combination rather than a variable holding a redirection:
    # the shell expands redirections before it expands variables, so `$REDIR`
    # would be an argument and not a redirection.
    if [ -n "$CLOSED" ] && [ -n "$ERR_CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >&- 2>&-
    elif [ -n "$ERR_CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >"$out" 2>&-
    elif [ -n "$ERR_FULL" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >"$out" 2>/dev/full
    elif [ -n "$CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >&- 2>"$err"
    elif [ -n "$TO_FULL" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >/dev/full 2>"$err"
    else
      # shellcheck disable=SC2086
      ( timeout -k 2 60 env $ENVV PATH="$bindir/$side" logname "$@" ) >"$out" 2>"$err"
    fi
    # On the very next line, before anything else runs — including a `[ ]`
    # test, whose own status would silently replace it. See tee-diff.sh, where
    # getting this wrong cost a full run.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  local o_out g_out o_msg g_msg
  o_out=$(render "$o_bin"); g_out=$(render "$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# The label is built before `compare` runs, because `compare` resets the knobs.
label_of() {
  printf 'logname %s%s%s%s%s%s' "$*" \
    "${ENVV:+  [$ENVV]}" "${TO_FULL:+  [>/dev/full]}" "${CLOSED:+  [>&-]}" \
    "${ERR_FULL:+  [2>/dev/full]}" "${ERR_CLOSED:+  [2>&-]}"
}
run_case() { local label; label=$(label_of "$@"); compare "$@"; report "$label"; }

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

# --- the answer --------------------------------------------------------------
run_case

# --- the environment is ignored ----------------------------------------------
# The whole reason this file exists. `su` sets `$LOGNAME`, so a `logname` that
# reads it is a second `whoami` and answers the wrong question.
ENVV="LOGNAME=zzz"; run_case
ENVV="USER=zzz"; run_case
ENVV="LOGNAME=zzz USER=zzz"; run_case
ENVV="LOGNAME= USER="; run_case
ENVV="-u LOGNAME -u USER"; run_case
ENVV="LOGNAME=root USER=root"; run_case

# --- operands ----------------------------------------------------------------
# There are none. The first extra one is named, and the rest are not.
run_case x
run_case a b
run_case ''
run_case -
run_case -- x
run_case -- --help

# --- the option errors -------------------------------------------------------
run_case -x
run_case -l
run_case --nope
run_case ---help
run_case --help=1
run_case --version=

# --- help and version --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The operand check happens after the whole scan, so an option still wins over
# an operand that precedes it.
xfail_case 'our --help omits the GNU project ancillary block' x --help
xfail_case 'our --version names SlateOS' --version --help
# Prefixes: `--h` and `--v` are unambiguous, there being only two long options.
xfail_case 'our --help omits the GNU project ancillary block' --hel
xfail_case 'our --version names SlateOS' --v

# --- standard output closed, and full ----------------------------------------
# Both were a silent exit 0 before `coreutils::stdfd`.
CLOSED=1; run_case
TO_FULL=1; run_case
CLOSED=1; run_case --help
TO_FULL=1; run_case --version
# The usage error never gets as far as a flush, so a closed stdout adds
# nothing to it: the operand complaint is the whole output.
CLOSED=1; run_case x

# --- standard *error* closed, and full ---------------------------------------
# gnulib's `close_stdout` closes descriptor 2 as well, and `_exit`s with
# `exit_failure` if that fails -- so a diagnostic that could not be delivered
# replaces the status the run had earned, silently. Ours reached this file
# exiting 134, `Aborted (core dumped)`: `eprintln!` panics on a failed write,
# and the panic message then fails to print for the same reason.
#
# `--help` and `--version` are left out on purpose: they are `xfail` on stdout
# already, and what this group asks about is the status alone.
ERR_FULL=1;   run_case
ERR_CLOSED=1; run_case
# A diagnostic was attempted and lost, so `exit_failure` replaces the status.
ERR_FULL=1;   run_case x
ERR_CLOSED=1; run_case x
ERR_FULL=1;   run_case --nope
ERR_CLOSED=1; run_case -q
ERR_CLOSED=1; run_case --help=1
# Neither descriptor left: the status is the only thing that carries anything.
CLOSED=1; ERR_CLOSED=1; run_case
CLOSED=1; ERR_CLOSED=1; run_case x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]

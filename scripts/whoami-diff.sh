#!/bin/sh
# whoami-diff.sh — compare our `whoami` against the real GNU one, inside WSL.
#
# ## What this is checking
#
# `whoami` has one job and no flags, which is exactly why the old
# implementation could be wrong for months without anyone noticing: it printed
# *something*, and the something was usually right. It read `$USER`, then
# `$LOGNAME`. GNU reads neither — it is documented as "same as `id -un`", which
# is `getpwuid(geteuid())`. The difference only shows when the environment
# disagrees with the kernel, which is precisely the situation a script calling
# `whoami` is trying to detect.
#
# So the cases here are grouped as:
#
#   * **the environment is ignored** — `USER=zzz LOGNAME=zzz whoami` must still
#     print the real account name. This is the defect the file was rewritten
#     for, and it is one line of this harness.
#   * **no name for the id** — under `unshare --map-user`, a uid with no
#     `/etc/passwd` entry. GNU says `cannot find name for user ID 31337` and
#     exits 1; the old code printed the number as though it were an answer.
#     Skipped, loudly, where user namespaces are not available.
#   * **the operands** — `whoami` takes none, and refuses the first extra one
#     with gnulib's *locale* `quote()`, so `‘x’` and not `'x'`.
#   * **the option errors** — an empty `getopt_long` string, so `-x` is
#     `invalid option -- 'x'` rather than an operand, and long-option prefixes
#     and `--opt=value` behave as glibc's getopt does.
#   * **the write errors** — `>&-` and `>/dev/full`, the sweep this file was
#     written for. Rust reopens a closed descriptor on /dev/null before `main`
#     and then reports writes to a closed one as successes, so both of these
#     were a silent exit 0 before `coreutils::stdfd`.
#
# ## Cases that differ on purpose
#
# Two kinds, every one recorded as `xfail`: `--help` omits the GNU project's
# ancillary block, as every converted utility here does, and `--version` names
# SlateOS.
#
# Run `OURS=/usr/bin/whoami ./scripts/whoami-diff.sh` to confirm the harness
# still discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG=whoami
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- knobs, reset before every case ------------------------------------------
# `ENVV` is a word list handed to `env`, so it can hold both assignments
# (`USER=zzz`) and `env`'s own flags (`-u USER`). `PRE` is a command prefix,
# for the one case that has to run under a different uid. `TO_FULL` and
# `CLOSED` are the two ways to reach a write error — they are not
# interchangeable: /dev/full fails every write, whereas a closed descriptor
# only fails one that was actually attempted.
ENVV=; PRE=; TO_FULL=; CLOSED=

reset_knobs() { ENVV=; PRE=; TO_FULL=; CLOSED=; }

# Bytes, not text: a login name is a field of `/etc/passwd` and may hold
# anything but `:` and a newline, so the comparison must not go through
# something that would normalise it.
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
    # `$ENVV` and `$PRE` are deliberately unquoted: both are word lists.
    if [ -n "$CLOSED" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 $PRE env $ENVV PATH="$bindir/$side" whoami "$@" ) >&- 2>"$err"
    elif [ -n "$TO_FULL" ]; then
      # shellcheck disable=SC2086
      ( timeout -k 2 60 $PRE env $ENVV PATH="$bindir/$side" whoami "$@" ) >/dev/full 2>"$err"
    else
      # shellcheck disable=SC2086
      ( timeout -k 2 60 $PRE env $ENVV PATH="$bindir/$side" whoami "$@" ) >"$out" 2>"$err"
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
  printf 'whoami %s%s%s%s%s' "$*" \
    "${PRE:+  [$PRE]}" "${ENVV:+  [$ENVV]}" \
    "${TO_FULL:+  [>/dev/full]}" "${CLOSED:+  [>&-]}"
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
# The whole reason this file exists. `whoami` is not `logname`: it reports what
# the kernel will enforce, not what the environment claims.
ENVV="USER=zzz"; run_case
ENVV="LOGNAME=zzz"; run_case
ENVV="USER=zzz LOGNAME=zzz"; run_case
ENVV="USER= LOGNAME="; run_case
ENVV="-u USER -u LOGNAME"; run_case
# `id -un` is the documented equivalent, and `$USER` moves neither of them.
ENVV="USER=root LOGNAME=root"; run_case

# --- no name for the id ------------------------------------------------------
# A uid with no `/etc/passwd` entry. `unshare --map-user` is the only way to
# reach one without being root; where user namespaces are unavailable the case
# is skipped rather than quietly turned into another copy of the first case.
if unshare --map-user=31337 -- true 2>/dev/null; then
  PRE="unshare --map-user=31337 --"; run_case
  PRE="unshare --map-user=31337 --"; CLOSED=1; run_case
else
  echo "whoami-diff: no user namespaces here; skipping the unknown-uid cases"
  echo "  (the old implementation printed the number instead of failing, so"
  echo "   this is the case that would catch a regression to it.)"
fi

# --- operands ----------------------------------------------------------------
# There are none. The first extra one is named, and the rest are not.
run_case x
run_case x y
run_case ''
run_case -
run_case -- x
run_case -- --help

# --- the option errors -------------------------------------------------------
run_case -x
run_case -u
run_case --nope
run_case ---help

# --- help and version --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The operand check happens after the whole scan, so an option still wins over
# an operand that precedes it.
xfail_case 'our --help omits the GNU project ancillary block' x --help
xfail_case 'our --version names SlateOS' --version --help
# Prefixes: `--h` and `--v` are unambiguous, there being only two long options.
xfail_case 'our --help omits the GNU project ancillary block' --h
xfail_case 'our --version names SlateOS' --v
# And a value given to a flag is refused rather than ignored — not an xfail,
# since the wording is glibc's and ours both.
run_case --help=1
run_case --version=

# --- standard output closed, and full ----------------------------------------
# Both were a silent exit 0 before `coreutils::stdfd`.
CLOSED=1; run_case
TO_FULL=1; run_case
CLOSED=1; run_case --help
TO_FULL=1; run_case --version
# The usage error never gets as far as a flush, so a closed stdout adds
# nothing to it: the operand complaint is the whole output.
CLOSED=1; run_case x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]

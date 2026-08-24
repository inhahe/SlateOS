#!/bin/bash
# echo-diff.sh — run our `echo` and GNU coreutils' `echo` side by side and
# report every case where they disagree.
#
# `echo` is the utility it is easiest to believe needs no testing, and the one
# where a difference is hardest to notice: it has no exit status worth reading,
# no diagnostics, and a shell builtin of the same name shadowing it in every
# interactive check you might make by hand. `type echo` in bash says `builtin`,
# so `echo -e '\x41'` at a prompt tells you nothing at all about the program in
# `/usr/bin`. This harness invokes the two binaries directly.
#
# ## How it runs
#
# Our `echo` is built *for Linux inside WSL* and compared against the GNU
# binary there, rather than building a Windows subject and reaching into WSL
# only for the reference — the same answer `cmp`, `tee`, `du`, `find` and `ls`
# use (`design-decisions.md` §374). The build lands in
# `$HOME/.cache/slateos-diff-target` inside WSL, shared with the others.
#
# Both binaries are reached through a symlink named `echo` in a directory that
# is the whole of `PATH` for that one invocation, so `argv[0]` is the bare word
# `echo` on both sides. That matters here more than for most: `echo`'s only
# diagnostic (`write error`) is prefixed with `argv[0]`, and `--help` prints it
# in the usage line, so a comparison run against `/usr/bin/echo` would report a
# difference in every one of those cases for a reason that is not about echo.
#
# ## Cases that differ on purpose
#
# Two, both recorded as `xfail`:
#
#   * `--help` omits the GNU project's ancillary block (`Report bugs to:` and
#     friends), as every converted utility here does.
#   * `--version` names SlateOS.
#
# Nothing else. In particular the write-error case is *not* an exception: GNU
# reaches it through gnulib's `close_stdout` atexit hook and we reach it by
# building the output and writing it once, but both print exactly
# `echo: write error: No space left on device` and exit 1.
#
# That holds for a *closed* standard output too (`echo hi >&-`), which is the
# harder half — Rust reopens a closed descriptor on /dev/null before `main` and
# then reports writes to a closed one as successes, so getting this right took
# `coreutils::stdfd` rather than an ordinary `write` call. Note that the answer
# depends on whether anything was owed: `echo hi >&-` is an error and
# `echo -n >&-` is not.
#
# Run `OURS=/usr/bin/echo ./scripts/echo-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# --- get ourselves into WSL --------------------------------------------------
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "echo-diff: no WSL on this host; skipping"
    exit 0
  fi
  here=$(cd "$(dirname "$0")" && pwd)
  if command -v cygpath >/dev/null 2>&1; then here=$(cygpath -m "$here"); fi
  inside=$(wsl wslpath -u "$here" 2>/dev/null) || {
    echo "echo-diff: could not map $here into WSL; skipping"
    exit 0
  }
  exec wsl -e env "OURS=${OURS:-}" "VERBOSE=${VERBOSE:-}" \
    bash "$inside/echo-diff.sh"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- the reference -----------------------------------------------------------
# `command -v echo` finds the *builtin* in bash, which is not what is being
# compared here. Ask for the file.
gnu_real=
for candidate in /usr/bin/echo /bin/echo; do
  [ -x "$candidate" ] && { gnu_real=$candidate; break; }
done
if [ -z "$gnu_real" ]; then
  echo "echo-diff: no GNU echo binary inside WSL; skipping"
  exit 0
fi

# --- the subject -------------------------------------------------------------
OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "echo-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  target_dir=$HOME/.cache/slateos-diff-target
  ( cd "$root" && cargo build -p coreutils --bin echo \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  OURS=$target_dir/x86_64-unknown-linux-gnu/debug/echo
fi
if [ ! -x "$OURS" ]; then
  echo "echo-diff: $OURS is not executable" >&2
  exit 1
fi
case $OURS in
  /*) ;;
  *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
esac

# echo produces no locale-dependent text of its own, but `write error` comes
# from `strerror`, which does. Fixed for the same reason as everywhere since
# §351.
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

# --- one name for both sides -------------------------------------------------
bindir=$(mktemp -d)
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/echo"
ln -s "$gnu_real" "$bindir/gnu/echo"
trap 'rm -rf "$bindir"' EXIT

# --- knobs, reset before every case ------------------------------------------
# `ENVV` is a `VAR=value` word placed in the environment of both sides, and
# exists for exactly one variable: `POSIXLY_CORRECT`, which changes echo more
# than any option does.
#
# `TO_FULL` sends stdout to /dev/full, which is one of the two ways to reach
# echo's one diagnostic.
#
# `CLOSED` is the other: it runs the command with standard output *closed*
# (`>&-`), which a shell can ask for and which the Rust runtime quietly refuses
# to allow — it reopens the descriptor on /dev/null before `main`, and then
# reports a write to a closed one as a full success. Together those turn
# `echo hi >&-` into a silent 0 where GNU prints `echo: write error: Bad file
# descriptor` and exits 1. The two knobs are not interchangeable: /dev/full
# fails every write, whereas a closed descriptor only fails one that was
# actually attempted, so `echo -n >&-` must still be a silent 0.
ENVV=; TO_FULL=; CLOSED=

reset_knobs() { ENVV=; TO_FULL=; CLOSED=; }

# Bytes, not text: an argument may hold anything but NUL, and a case whose
# whole point is `\xff` must not be compared through something that would
# normalise it.
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
    if [ -n "$CLOSED" ]; then
      timeout -k 2 60 env ${ENVV:+"$ENVV"} PATH="$bindir/$side" echo "$@" >&- 2>"$err"
    elif [ -n "$TO_FULL" ]; then
      timeout -k 2 60 env ${ENVV:+"$ENVV"} PATH="$bindir/$side" echo "$@" >/dev/full 2>"$err"
    else
      timeout -k 2 60 env ${ENVV:+"$ENVV"} PATH="$bindir/$side" echo "$@" >"$out" 2>"$err"
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
run_case() { local label="echo $*${ENVV:+  [$ENVV]}${TO_FULL:+  [>/dev/full]}${CLOSED:+  [>&-]}"; compare "$@"; report "$label"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  local label="echo $*"
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

# =============================================================================
# --- the basics --------------------------------------------------------------
run_case
run_case hello
run_case a b c
run_case a '' b
run_case 'two  spaces'
run_case '   leading and trailing   '

# --- what counts as an option ------------------------------------------------
run_case -n hi
run_case -n
run_case -e hi
run_case -E hi
run_case hi -n
run_case - -n
run_case -- -n
run_case -x -n
run_case -en1 'a\tb'
run_case '' -n
run_case -n -- -e 'a\tb'

# Bundles of any length, and the last of -e/-E winning within and across words.
run_case -ne 'a\tb'
run_case -en 'a\tb'
run_case -neE 'a\tb'
run_case -nEe 'a\tb'
run_case -nnnn hi
run_case -e -E 'a\tb'
run_case -E -e 'a\tb'
run_case -eE 'a\tb'
run_case -Ee 'a\tb'
run_case -e -e -n 'a\tb'

# A word that looks like an option but is not ends the scan for good.
run_case -e hi -n
run_case -e -x -n 'a\tb'

# --- the two long options ----------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# Only as the entire command line; with anything else they are text, and the
# scan stops on them so a later -n is text too.
run_case --help x
run_case --version x
run_case x --help
run_case -n --help
# Never abbreviated: upstream parses these by hand and says why.
run_case --hel
run_case --vers
run_case --helpx
run_case ---help

# --- escapes, off ------------------------------------------------------------
run_case 'a\tb'
run_case 'a\nb'
run_case '\\'
run_case 'a\cb' second

# --- escapes, on -------------------------------------------------------------
run_case -e 'a\tb'
run_case -e '\a\b\e\f\n\r\t\v'
run_case -e 'back\\slash'
run_case -e 'a\qb'
run_case -e 'trailing\'
run_case -e '\'
run_case -e 'a\\\tb'

# \c ends the program: no newline, and nothing after it anywhere.
run_case -e 'a\cb'
run_case -e 'a\cb' second third
run_case -e '\c'
run_case -e first '\c' third
run_case -ne 'a\cb'

# Hex: one or two digits, and not an escape at all without one.
run_case -e '\x41'
run_case -e '\x4'
run_case -e '\x41B'
run_case -e '\xfF'
run_case -e '\xz'
run_case -e '\x'
run_case -e 'a\x'
run_case -e '\x0'

# Octal, with and without the documented leading zero, at most three digits,
# wrapping at a byte.
run_case -e '\101'
run_case -e '\0101'
run_case -e '\1011'
run_case -e '\01011'
run_case -e 'a\0b'
run_case -e '\08'
run_case -e '\0777'
run_case -e '\777'
run_case -e '\7'
run_case -e '\8'
run_case -e '\9'
run_case -e '\0'

# --- POSIXLY_CORRECT ---------------------------------------------------------
# Escapes without -e, options as text, and the one exception for a leading -n.
ENVV='POSIXLY_CORRECT=1'; run_case 'a\tb'
ENVV='POSIXLY_CORRECT=1'; run_case -e x
ENVV='POSIXLY_CORRECT=1'; run_case -E 'a\tb'
ENVV='POSIXLY_CORRECT=1'; run_case --help
ENVV='POSIXLY_CORRECT=1'; run_case --version
ENVV='POSIXLY_CORRECT=1'; run_case -n x
ENVV='POSIXLY_CORRECT=1'; run_case -n -E 'a\tb'
ENVV='POSIXLY_CORRECT=1'; run_case -n -e 'a\tb'
ENVV='POSIXLY_CORRECT=1'; run_case -n 'a\cb' second
ENVV='POSIXLY_CORRECT=1'; run_case x -n
ENVV='POSIXLY_CORRECT='; run_case 'a\tb'

# --- arguments that are not text ---------------------------------------------
run_case $'\xff\xfe'
run_case -e $'\xff\\t\xfe'
run_case $'line\nbreak'
run_case $'tab\there'
run_case -n $'\xff'

# --- the one diagnostic ------------------------------------------------------
TO_FULL=1; run_case hi
TO_FULL=1; run_case -n hi
TO_FULL=1; run_case -e 'a\cb'
TO_FULL=1; run_case

# --- standard output closed --------------------------------------------------
# The distinction the buffering exists for: a case with something to write is
# an error, a case with nothing to write is not. `echo -n` and `echo -e '\c'`
# both produce no bytes at all, so nothing is ever owed to the descriptor and
# both must exit 0 in silence — even though writing to it would have failed.
CLOSED=1; run_case hi
CLOSED=1; run_case
CLOSED=1; run_case -n hi
CLOSED=1; run_case -n
CLOSED=1; run_case -e 'a\cb'
CLOSED=1; run_case -e '\c'
CLOSED=1; run_case --help
CLOSED=1; run_case --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0

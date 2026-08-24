# Shared preamble for a differential harness that runs both sides inside WSL.
#
# Sourced, not run. It does the six things every such harness was doing for
# itself, identically, in about fifty lines apiece:
#
#   1. re-exec itself inside WSL if it is not already there
#   2. find the repository root as WSL sees it
#   3. find the GNU reference, and skip the run rather than pass wrongly
#   4. build our binary for `x86_64-unknown-linux-gnu`
#   5. fix the locale
#   6. put both binaries behind one name, so `argv[0]` matches on both sides
#
# ## Why any of it
#
# Two reasons, and both are load-bearing rather than convenience.
#
# The reference has to be glibc's. MSYS2 is a Cygwin derivative and its getopt
# is not glibc's -- `unknown option -- x` against `invalid option -- 'x'` --
# so a harness that compares against MSYS2 certifies wording no GNU/Linux
# system prints. `sort-diff.sh` did that for eight cases and passed the whole
# time (known-issues.md ->
# TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE).
#
# The subject has to be a Linux binary. Some of what these utilities do exists
# only there: `coreutils::stdfd`, which is what makes `prog >&-` behave, is
# `#[cfg(target_os = "linux")]` because the two runtime lies it undoes are
# undone with `.init_array` and raw `write(2)`. A Windows build cannot exercise
# a line of it, so a Windows-hosted harness cannot catch a regression in it.
#
# The build lands in `$HOME/.cache/slateos-diff-target` inside WSL, shared
# between harnesses and kept out of the repository's `target/` so the Linux and
# Windows builds do not invalidate each other (design-decisions.md §374).
#
# ## Using it
#
# Set the knobs, then source it, before anything else in the harness:
#
#     DIFF_PROG=cat
#     . "$(dirname "$0")/diff-wsl.sh"
#
# | knob | default | meaning |
# |---|---|---|
# | `DIFF_PROG`      | *required* | the utility's name: used for messages, for finding the reference, and as the one name both binaries are reached by |
# | `DIFF_PKG`       | `coreutils` | the cargo package to build from |
# | `DIFF_BINS`      | `$DIFF_PROG` | the `--bin` names to build; more than one for a harness that compares a family |
# | `DIFF_FORWARD`   | (none) | extra environment variable names to carry across the re-exec, beyond `OURS` and `VERBOSE` |
# | `DIFF_REF`       | (none) | candidate paths for the reference, tried in order, instead of looking on `PATH`. `echo` needs this: `command -v echo` finds the shell builtin, which is not what is being compared |
# | `DIFF_NEED`      | (none) | other commands that must exist inside WSL, or the run is skipped rather than run without them |
# | `DIFF_NO_REF`    | (unset) | do not look for a reference; the harness finds its own |
# | `DIFF_NO_BINDIR` | (unset) | do not build the `PATH` directories; the harness makes its own |
#
# Afterwards it has set:
#
# | name | |
# |---|---|
# | `root`       | the repository root |
# | `target_dir` | the shared Linux target directory |
# | `OURS`       | our binary, absolute (single `DIFF_BINS` only) |
# | `gnu_real`   | the reference binary, absolute (unless `DIFF_NO_REF`) |
# | `DIFF_TMP`   | a scratch directory, removed on exit |
# | `bindir`     | `$DIFF_TMP/bin`, holding `ours/$DIFF_PROG` and `gnu/$DIFF_PROG` |
# | `diff_ours`  | `diff_ours NAME` -> the path of another built binary |
#
# A harness's own fixtures belong under `$DIFF_TMP`, so that the one `EXIT`
# trap set here cleans up everything. Setting a second `trap ... EXIT` would
# replace this one, not add to it, and leak the scratch directory every run;
# extend `diff_cleanup` instead if there is more to do.
#
# `OURS=/usr/bin/<prog>` overrides the build with the reference itself, which
# is how a harness is checked for still being able to tell the two apart: it
# should then report every xfail as an XPASS and nothing else.

if [ -z "${DIFF_PROG:-}" ]; then
  echo "diff-wsl.sh: DIFF_PROG is not set" >&2
  exit 1
fi
: "${DIFF_PKG:=coreutils}"
: "${DIFF_BINS:=$DIFF_PROG}"
: "${DIFF_FORWARD:=}"
: "${DIFF_REF:=}"
: "${DIFF_NEED:=}"

# MSYS would rewrite an argument that looks like a path on its way to `wsl`.
export MSYS2_ARG_CONV_EXCL='*'

# --- 1. get ourselves into WSL ------------------------------------------------
# `$0` may be an MSYS path (`/d/visual studio projects/...`) or a Windows one.
# `wslpath` translates whatever it is; it exists only inside WSL, which is also
# how we tell we are already there.
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no WSL on this host; skipping (ours is a unix-only binary)"
    exit 0
  fi
  diff_here=$(cd "$(dirname "$0")" && pwd)
  # `wsl wslpath` converts a *Windows* path; MSYS's own `/d/...` form is not
  # one, so hand over the mixed form cygpath produces, which WSL understands.
  if command -v cygpath >/dev/null 2>&1; then diff_here=$(cygpath -m "$diff_here"); fi
  diff_inside=$(wsl wslpath -u "$diff_here" 2>/dev/null) || {
    echo "$DIFF_PROG-diff: could not map $diff_here into WSL; skipping"
    exit 0
  }
  # The forwarded variables are built into the positional parameters because
  # this file is sourced by a `sh` that may have no arrays. Nothing returns
  # from here, so clobbering them is free.
  set --
  for diff_v in OURS VERBOSE $DIFF_FORWARD; do
    eval "set -- \"\$@\" \"$diff_v=\${$diff_v:-}\""
  done
  exec wsl -e env "$@" bash "$diff_inside/$(basename "$0")"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- 5. the locale ------------------------------------------------------------
# Fixed under UTF-8, as everywhere since §351: getopt renders an unknown or
# ambiguous option with directional single quotes under a UTF-8 locale and
# ASCII apostrophes under `C`, so the whole option-error family would disagree
# for a reason unrelated to the program being tested. `strerror` is
# locale-dependent too, which is why even a program with no text of its own
# needs this. `C.UTF-8` is present on every glibc build; a named territory
# locale is not.
export LC_ALL=C.UTF-8

# --- 3. the reference ---------------------------------------------------------
for diff_cmd in $DIFF_NEED; do
  if ! command -v "$diff_cmd" >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no '$diff_cmd' inside WSL; skipping"
    echo "  the cases that need it would otherwise be green and meaningless."
    exit 0
  fi
done

gnu_real=
if [ -z "${DIFF_NO_REF:-}" ]; then
  if [ -n "$DIFF_REF" ]; then
    for diff_cand in $DIFF_REF; do
      [ -x "$diff_cand" ] && { gnu_real=$diff_cand; break; }
    done
  else
    gnu_real=$(command -v "$DIFF_PROG" 2>/dev/null) || gnu_real=
  fi
  if [ -z "$gnu_real" ]; then
    echo "$DIFF_PROG-diff: no GNU $DIFF_PROG inside WSL; skipping"
    exit 0
  fi
fi

# --- 4. the subject -----------------------------------------------------------
target_dir=$HOME/.cache/slateos-diff-target

# The path of one of the binaries built above.
diff_ours() {
  printf '%s/x86_64-unknown-linux-gnu/debug/%s' "$target_dir" "$1"
}

OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "$DIFF_PROG-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  # Built every run, for the reason `diff-subject.sh` spells out at length: a
  # harness that merely *runs* a path measures whatever was written there last,
  # which need not be current and need not even be this crate. One `cargo
  # build` for the whole family rather than one per binary, so the output does
  # not read as though something were rebuilt between two halves of a run.
  diff_args=
  for diff_b in $DIFF_BINS; do diff_args="$diff_args --bin $diff_b"; done
  # shellcheck disable=SC2086
  ( cd "$root" && cargo build -p "$DIFF_PKG" $diff_args \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  case $DIFF_BINS in
    *' '*) ;;   # a family: the harness picks binaries with `diff_ours`
    *) OURS=$(diff_ours "$DIFF_BINS") ;;
  esac
fi
if [ -n "$OURS" ]; then
  if [ ! -x "$OURS" ]; then
    echo "$DIFF_PROG-diff: $OURS is not executable" >&2
    exit 1
  fi
  # Absolute, because the symlinks below are followed from a different
  # directory than the one the harness was started in.
  case $OURS in
    /*) ;;
    *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
  esac
fi

# --- 6. one scratch directory, and one name for both sides --------------------
DIFF_TMP=$(mktemp -d)

# Extend this rather than setting a second `EXIT` trap, which would replace
# this one. The `chmod` is for harnesses whose fixtures include an unreadable
# directory: `rm -rf` cannot descend into one.
diff_cleanup() {
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
}
trap diff_cleanup EXIT

bindir=$DIFF_TMP/bin
if [ -z "${DIFF_NO_BINDIR:-}" ]; then
  # Each binary is reached through a symlink named `$DIFF_PROG`, in a directory
  # that is the whole of `PATH` for that one invocation, so `argv[0]` is the
  # bare word on both sides and the `prog: ` prefix on every diagnostic matches.
  mkdir -p "$bindir/ours" "$bindir/gnu"
  ln -s "$OURS" "$bindir/ours/$DIFF_PROG"
  ln -s "$gnu_real" "$bindir/gnu/$DIFF_PROG"
fi

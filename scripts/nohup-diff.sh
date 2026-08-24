#!/usr/bin/env bash
# Differential test: our `nohup` against GNU coreutils'.
#
# ## Why this harness needs a terminal, and the others do not
#
# Every branch that makes `nohup` `nohup` is guarded by `isatty`. Run from a
# pipe — which is how a test harness normally runs anything — the program
# redirects nothing, says nothing, and creates no `nohup.out`, so a harness
# built like `cut-diff.sh` would exercise the argument parser and *nothing
# else*, while reporting a full green column. That is the failure this file is
# shaped to avoid: the interesting half of the cases below run the program
# under a pseudo-terminal via `script -qec`, so `isatty` answers yes and the
# redirections actually happen.
#
# ## What is compared
#
# Not just stdout/stderr/status. `nohup`'s real output is a *file* — its name,
# its mode and its contents — so every case also compares a snapshot of the
# directory it ran in: each path with its octal mode and size, plus the bytes
# of every file. A port that printed the right sentence and created
# `nohup.out` world-readable, or in the wrong directory, or empty, would pass
# a text-only comparison and fail this one.
#
# Both sides run in their own fresh directory, so `nohup.out` from one case
# cannot be appended to by the next.
#
# ## Why both sides run inside WSL
#
# The same reasons as `cmp-diff.sh`, `tee-diff.sh` and `digest-diff.sh`, whose
# headers spell them out, plus one specific to this program: it is built on
# `isatty`, `dup2` and `exec`, none of which the Windows host has. The build
# lands in `$HOME/.cache/slateos-diff-target` inside WSL, shared with the other
# harnesses (`design-decisions.md` §374).
#
# ## Cases that differ on purpose
#
# Two, both the family's: `--help` omits the GNU project's `Report bugs to:`
# block, and `--version` names SlateOS.
#
# Run `OURS=/usr/bin/nohup ./scripts/nohup-diff.sh` to confirm the harness
# still discriminates: it should report every xfail as XPASS and nothing else.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# --- get ourselves into WSL --------------------------------------------------
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "nohup-diff: no WSL on this host; skipping (ours is a unix-only binary)"
    exit 0
  fi
  here=$(cd "$(dirname "$0")" && pwd)
  if command -v cygpath >/dev/null 2>&1; then here=$(cygpath -m "$here"); fi
  inside=$(wsl wslpath -u "$here" 2>/dev/null) || {
    echo "nohup-diff: could not map $here into WSL; skipping"
    exit 0
  }
  exec wsl -e env "OURS=${OURS:-}" "VERBOSE=${VERBOSE:-}" \
    bash "$inside/nohup-diff.sh"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- the reference -----------------------------------------------------------
if ! command -v nohup >/dev/null 2>&1; then
  echo "nohup-diff: no GNU nohup inside WSL; skipping"
  exit 0
fi
if ! command -v script >/dev/null 2>&1; then
  echo "nohup-diff: no util-linux 'script' inside WSL; skipping"
  echo "  every isatty-guarded branch needs it; a run without it would be"
  echo "  green and meaningless."
  exit 0
fi

# --- the subject -------------------------------------------------------------
OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "nohup-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  target_dir=$HOME/.cache/slateos-diff-target
  ( cd "$root" && cargo build -p coreutils --bin nohup \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  OURS=$target_dir/x86_64-unknown-linux-gnu/debug/nohup
fi
if [ ! -x "$OURS" ]; then
  echo "nohup-diff: $OURS is not executable" >&2
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
# Each binary is reached through a symlink called `nohup`, in a directory
# *prepended* to `PATH` — prepended rather than replacing it, because unlike
# the other harnesses this one runs real commands (`true`, `sh`, `cat`) through
# the program under test, and they have to be findable. `argv[0]` is the bare
# word `nohup` on both sides either way, so the `nohup: ` prefix matches.
gnu_real=$(command -v nohup)
bindir=$(mktemp -d)
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/nohup"
ln -s "$gnu_real" "$bindir/gnu/nohup"

work=$(mktemp -d)
cleanup() {
  # A case that made a directory unwritable on purpose has already restored it,
  # but a case that died in the middle may not have.
  chmod -R u+rwX "$work" 2>/dev/null
  rm -rf "$bindir" "$work"
}
trap cleanup EXIT

case_no=0

# --- what a case leaves behind -----------------------------------------------
# The name, the octal mode and the size of everything in the directory. Mode is
# in the snapshot because `nohup.out` being 0600 is a security property, not a
# cosmetic one, and size because an empty `nohup.out` and a full one are the
# difference between the output arriving and being lost.
snapshot() {
  ( cd "$1" 2>/dev/null && find . -mindepth 1 -printf '%P %m %s\n' 2>/dev/null \
      | LC_ALL=C sort )
}

# And the bytes, so a `nohup.out` with the right size and the wrong contents is
# still caught.
contents() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\n' 2>/dev/null | LC_ALL=C sort | while read -r f; do
      printf '== %s\n' "$f"
      cat -- "$f"
      printf '\n'
    done )
}

# --- knobs, reset after every case -------------------------------------------

# Shell run inside the case directory before the program, for the cases that
# need a read-only directory or a pre-existing `nohup.out`.
SETUP=
# Shell run inside the case directory afterwards, to undo `SETUP` far enough
# that the snapshot and the cleanup can read it.
TEARDOWN=
# Compare *which* of help/version/nothing came out rather than its full text.
# Used by the abbreviation cases, whose question is which option a prefix
# resolves to, not what that option prints — comparing the text there would
# fail on the difference already recorded as an xfail below, for every case,
# which is how a known difference stops being a record and becomes noise.
KIND=
reset_knobs() { SETUP=; TEARDOWN=; KIND=; }

# The two sides run in two different directories, and some messages name the
# directory they ran in — the `$HOME` fallback prints the whole path it opened.
# Comparing those raw would fail on the one thing that is *supposed* to differ,
# so each side's own directory is replaced by a marker first. The replacement is
# per side, not a common prefix, which is what makes it a comparison rather than
# a blanket erasure: a path pointing somewhere other than that side's own
# directory survives and shows up as a difference.
scrub() { sed -e "s|$1|<DIR>|g"; }

classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: nohup }" != "$first" ]; then echo help
  elif [ "${first#nohup \(}" != "$first" ]; then echo version
  else echo other
  fi
}

# --- running one side --------------------------------------------------------

# No terminal anywhere: stdin from /dev/null, stdout and stderr to files. This
# is the parser-and-exec half of the program.
run_plain() {
  local side=$1 dir=$2 out=$3 err=$4 rcf=$5; shift 5
  mkdir -p "$dir"
  [ -n "$SETUP" ] && ( cd "$dir" && eval "$SETUP" ) >/dev/null 2>&1
  # The subshell's *own* stderr is discarded — the program's already goes to
  # `$err`, so all that is left here is the shell announcing `Terminated` when a
  # command kills itself, which the SIGTERM case below does on purpose.
  ( cd "$dir" && PATH="$bindir/$side:$PATH" \
      timeout -k 2 30 nohup "$@" </dev/null >"$out" 2>"$err" ) 2>/dev/null
  echo $? >"$rcf"
  [ -n "$TEARDOWN" ] && ( cd "$dir" && eval "$TEARDOWN" ) >/dev/null 2>&1
  return 0
}

# All three descriptors on a pseudo-terminal, with the snippet free to redirect
# whichever of them the case is about. The snippet's own status is echoed into
# the captured text because `script`'s status is `script`'s.
run_pty() {
  local side=$1 dir=$2 out=$3 snippet=$4
  mkdir -p "$dir"
  [ -n "$SETUP" ] && ( cd "$dir" && eval "$SETUP" ) >/dev/null 2>&1
  ( cd "$dir" && PATH="$bindir/$side:$PATH" \
      timeout -k 2 30 script -qec "$snippet"' ; echo "rc=$?"' /dev/null ) \
    >"$out" 2>&1
  [ -n "$TEARDOWN" ] && ( cd "$dir" && eval "$TEARDOWN" ) >/dev/null 2>&1
  # A pty turns every \n into \r\n; that is the terminal discipline, not the
  # program, and comparing it would only make the reports unreadable.
  tr -d '\r' <"$out" >"$out.clean" && mv "$out.clean" "$out"
  return 0
}

# --- comparing the two sides -------------------------------------------------

# Fills AGREED and REPORT from two directories and two captures.
judge() {
  local o_dir=$1 g_dir=$2 o_txt=$3 g_txt=$4 o_extra=$5 g_extra=$6 label=$7
  local o_snap g_snap o_body g_body o_show g_show
  o_snap=$(snapshot "$o_dir"); g_snap=$(snapshot "$g_dir")
  o_body=$(contents "$o_dir" | scrub "$o_dir"); g_body=$(contents "$g_dir" | scrub "$g_dir")
  if [ -n "$KIND" ]; then
    o_show="class $(classify "$o_txt")"; g_show="class $(classify "$g_txt")"
  else
    o_show=$(scrub "$o_dir" <"$o_txt"); g_show=$(scrub "$g_dir" <"$g_txt")
  fi
  o_extra=$(printf '%s' "$o_extra" | scrub "$o_dir")
  g_extra=$(printf '%s' "$g_extra" | scrub "$g_dir")

  if [ "$o_show" = "$g_show" ] && [ "$o_extra" = "$g_extra" ] \
     && [ "$o_snap" = "$g_snap" ] && [ "$o_body" = "$g_body" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n        %s\n        tree{%s} files{%s}\n  gnu : %s\n        %s\n        tree{%s} files{%s}' \
    "$(printf '%s' "$o_extra" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_snap" | tr '\n' '|')" "$(printf '%s' "$o_body" | tr '\n' '|')" \
    "$(printf '%s' "$g_extra" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_snap" | tr '\n' '|')" "$(printf '%s' "$g_body" | tr '\n' '|')")
  LABEL=$label
}

compare_plain() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no g_rc=$work/gr$case_no
  # `$out`/`$err` must be absolute: `run_plain` opens them after `cd`.
  run_plain ours "$o_dir" "$o_out" "$o_err" "$o_rc" "$@"
  run_plain gnu  "$g_dir" "$g_out" "$g_err" "$g_rc" "$@"
  local o_txt=$work/ot$case_no g_txt=$work/gt$case_no
  cat "$o_out" >"$o_txt"; cat "$g_out" >"$g_txt"
  judge "$o_dir" "$g_dir" "$o_txt" "$g_txt" \
    "rc=$(cat "$o_rc") err{$(cat "$o_err")}" \
    "rc=$(cat "$g_rc") err{$(cat "$g_err")}" \
    "nohup $*"
  reset_knobs
}

compare_pty() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  run_pty ours "$o_dir" "$o_out" "$1"
  run_pty gnu  "$g_dir" "$g_out" "$1"
  judge "$o_dir" "$g_dir" "$o_out" "$g_out" '' '' "[pty] $1"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case()     { compare_plain "$@"; report; }
run_pty_case() { compare_pty "$1"; report; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare_plain "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "nohup-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Usage errors
# =============================================================================
# All of these are 125 — nohup's own status, distinct from anything the command
# could have returned.

run_case
run_case --
run_case -x ls
run_case -xy ls
run_case --nope ls
run_case --help=x
run_case --version=x
run_case --=x                 # names every long option, in table order

# =============================================================================
# 2. Running the command
# =============================================================================
# With no terminal anywhere, nohup redirects nothing and creates nothing: the
# only observable effect is that the command ran and its status came back.

run_case true
run_case false
run_case sh -c 'exit 42'
run_case sh -c 'exit 0'
run_case echo hi
run_case sh -c 'echo out; echo err >&2'
run_case cat /etc/hostname
run_case printf '[%s]' a b c

# The status split that the old version guessed at.
run_case /nope/nope                              # 127: not found
SETUP='printf "#!/bin/sh\necho ran\n" > ne; chmod 644 ne'
run_case ./ne                                    # 126: found, not executable
SETUP='mkdir -p adir'
run_case ./adir                                  # 126: found, not a program

# =============================================================================
# 3. Everything after the command belongs to the command
# =============================================================================
# The `+` in SHORT_OPTIONS. Without it these would be nohup's own options and
# would be rejected — and the option most likely to be typed after a command is
# exactly `--help`.

run_case printf '[%s]' a -n
run_case echo -n hi
run_case echo --help
run_case echo --version
run_case sh -c 'echo "$1"' sh --nope
run_case -- -x                                   # the command is named `-x`
SETUP='printf "#!/bin/sh\necho iam \$0\n" > -x; chmod 755 -x'
TEARDOWN='true'
run_case -- ./-x

# =============================================================================
# 4. Bytes that are not valid UTF-8
# =============================================================================
# The finding that brought this program up for conversion: the old version held
# argv as `Vec<String>` and panicked here before doing anything at all.

run_case "$(printf 'na\377me')"                  # 127, and the name is quoted
run_case printf '[%s]' "$(printf 'na\377me')"    # passed through untouched
run_case printf '[%s]' "$(printf '\377')" "$(printf '\200\201')"

# =============================================================================
# 5. The thing it is named for
# =============================================================================
# A shell inherits an ignored signal as ignored, so a child that hangs itself up
# and lives is a child that was handed SIG_IGN for SIGHUP. The old version
# installed nothing at all, so this is the case that would have caught it.

run_case sh -c 'kill -HUP $$; echo survived'
run_case sh -c 'kill -HUP $$; kill -HUP $$; echo survived twice'
# And a signal that is *not* ignored still works, so the disposition was set for
# SIGHUP specifically rather than for everything.
run_case sh -c 'kill -TERM $$; echo should not get here'

# =============================================================================
# 6. Abbreviations
# =============================================================================
# Which option a prefix resolves to, not what that option prints — see `KIND`.

KIND=1; run_case --h
KIND=1; run_case --he
KIND=1; run_case --hel
KIND=1; run_case --v
KIND=1; run_case --ver
KIND=1; run_case --version

# =============================================================================
# 7. --help and --version
# =============================================================================

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 8. Under a terminal — where the program actually does something
# =============================================================================
# Everything above ran with all three descriptors on pipes or files, which is
# the case in which `nohup` deliberately does nothing. From here on the
# descriptors are a pty and the redirections happen.

run_pty_case 'nohup true'
run_pty_case 'nohup echo hi'
run_pty_case 'nohup sh -c "echo out; echo err >&2"'
run_pty_case 'nohup sh -c "exit 42"'

# The message depends on *which* descriptors are terminals, and the rule is not
# the obvious one: the fact that input is being ignored is folded into whatever
# other message is printed, and stands alone only when there is no other. The
# first draft of this port said "ignoring input" whenever stdin was a terminal
# and these four cases are what caught it.
run_pty_case 'nohup true > out.txt'                 # stdin+stderr tty
run_pty_case 'nohup true 2> err.txt'                # stdin+stdout tty
run_pty_case 'nohup true > out.txt 2> err.txt'      # stdin only
run_pty_case 'nohup true < /dev/null'               # stdout+stderr tty
run_pty_case 'nohup true < /dev/null 2> err.txt'    # stdout only
run_pty_case 'nohup true < /dev/null > out.txt'     # stderr only
run_pty_case 'nohup true < /dev/null > out.txt 2> err.txt'   # none

# `--help` says it redirects stdin "from an unreadable file", and means it: the
# child's reads must *fail*, not return end-of-file. A read-only /dev/null
# would make `cat` succeed on empty input, which looks like a command that ran
# fine on nothing.
run_pty_case 'nohup cat'
run_pty_case 'nohup sh -c "head -c 5 /dev/stdin; echo rc=\$?"'

# `nohup COMMAND > FILE` — the line in --help that the old version made a lie
# by redirecting to nohup.out unconditionally.
run_pty_case 'nohup echo hi > mine.txt'

# A closed stdout, which is not the same as a redirected one: nohup.out is
# opened anyway when stderr is a terminal, because the dup2 needs something on
# descriptor 1 to copy from.
run_pty_case 'nohup true >&-'
run_pty_case 'nohup true >&- 2> err.txt'
run_pty_case 'nohup true <&-'
run_pty_case 'nohup true <&- >&- 2>&-'

# A diagnostic that cannot be delivered is itself a failure: the messages are
# the only record of where the output went, so nohup refuses to run the command.
run_pty_case 'nohup true 2>&-'
run_pty_case 'nohup true < /dev/null > out.txt 2>&-'
if [ -w /dev/full ]; then
  run_pty_case 'nohup true 2>/dev/full'
  run_pty_case 'nohup true < /dev/null > out.txt 2>/dev/full'
  run_pty_case 'nohup /nope/nope < /dev/null > out.txt 2>/dev/full'
else
  echo "note: no writable /dev/full; the unwritable-stderr cases did not run" >&2
fi
run_pty_case 'nohup /nope/nope < /dev/null > out.txt 2>&-'

# The exec diagnostic has to reach the terminal, not the nohup.out that was
# just put on descriptor 1 — otherwise it lands where nobody is looking.
run_pty_case 'nohup /nope/nope'
run_pty_case 'nohup "$(printf "na\377me")"'

# =============================================================================
# 9. nohup.out itself
# =============================================================================

# Mode 0600 at creation, and forced there rather than merely requested: an
# ordinary open would be filtered through the inherited umask, so under
# `umask 0200` it would land at 0400 — a file its own owner cannot append to.
run_pty_case 'nohup true'
run_pty_case 'umask 000; nohup true'
run_pty_case 'umask 077; nohup true'
run_pty_case 'umask 0200; nohup true'
run_pty_case 'umask 0466; nohup true'

# The mode applies only at creation: an existing file keeps what it had, and
# output is appended to what is already there.
SETUP='printf "old\n" > nohup.out; chmod 644 nohup.out'
run_pty_case 'nohup echo new'

# The $HOME fallback, and which path the message names.
SETUP='mkdir -p ro home; chmod 555 ro'
TEARDOWN='chmod 755 ro'
run_pty_case 'cd ro && HOME="$PWD/../home" nohup true'

# Both fail: two messages, one per path tried, because the reader would
# otherwise not know $HOME had been tried at all.
SETUP='mkdir -p ro home; chmod 555 ro home'
TEARDOWN='chmod 755 ro home'
run_pty_case 'cd ro && HOME="$PWD/../home" nohup true'

# No $HOME to fall back to: one message.
SETUP='mkdir -p ro; chmod 555 ro'
TEARDOWN='chmod 755 ro'
run_pty_case 'cd ro && env -u HOME nohup true'

# An empty $HOME is not a directory name; it must not become "/nohup.out".
SETUP='mkdir -p ro; chmod 555 ro'
TEARDOWN='chmod 755 ro'
run_pty_case 'cd ro && HOME= nohup true'

# =============================================================================
# 10. Hangup immunity, under a terminal this time
# =============================================================================

run_pty_case 'nohup sh -c "kill -HUP \$\$; echo survived"'

# The wording is the family's, not this harness's own: `scripts/all-diff.sh`
# decides green by matching " 0 differed" in the tail line, so a summary that
# said "0 failed" would be reported as a failing harness forever.
printf '\nnohup: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0

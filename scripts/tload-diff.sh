#!/bin/bash
# tload-diff.sh -- run our `tload` and procps-ng's side by side and report every
# case where they disagree.
#
# ## The reference
#
# Ubuntu's /usr/bin/tload, procps 4.0.4-4ubuntu3.2; none of Debian's or
# Ubuntu's patches touch `src/tload.c`.
#
# ## How a run is captured
#
# tload never ends: it draws a frame every `-d` seconds until it is killed.
# `scripts/tload-frames.py` runs it, reads a chosen number of frames, and
# kills it -- from a pipe, which has no window size, so 80 columns of 25 rows;
# or from a pseudo-terminal of a chosen size, named as tload's tty operand,
# which it may also resize between two frames, sending `SIGWINCH`. What was
# drawn is compared byte for byte, with standard error and how the run ended.
#
# `/proc/loadavg` is pinned: each side runs in a user and mount namespace with
# a fixture bound over it, as `uptime-diff.sh` pins it, so both draw the same
# figures in every frame. A run whose load file is to be missing has an empty
# directory bound over `/proc` instead.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS; an empty `-s` or `-d` is refused without the
# reason upstream appends, which is whatever `errno` glibc's start-up left
# (`free`'s divergence 3); and a column whose height is not a finite number
# -- a load of `inf` or `nan`, or `-s inf`, `-s nan`, `-s 1e308` -- is drawn
# empty, where upstream never finishes the frame and spins until it is
# killed. Those runs are given 3 seconds rather than the helper's 90.
#
# Run `OURS=/usr/bin/tload ./scripts/tload-diff.sh` to check that the harness
# still discriminates: every case that differs on purpose should then be
# reported as no longer differing, and nothing else.
set -u

DIFF_PROG='tload'
DIFF_NEED='timeout python3 unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

helper=$root/scripts/tload-frames.py
pass=0; fail=0; xfail=0; xpass=0

if ! unshare -mUr sh -c "mount --bind /dev/null /proc/loadavg" 2>/dev/null; then
  echo "tload-diff: cannot pin /proc/loadavg in a namespace here; skipping"
  exit 0
fi
mkdir -p "$DIFF_TMP/emptydir"

LOAD='0.50 0.40 0.30 1/100 1234'
NOPROC=
reset_knobs() { LOAD='0.50 0.40 0.30 1/100 1234'; NOPROC=; }

# side SIDE MODE... -- ARGS...: tload-frames.py's run of one side.
side() {
  local s=$1; shift
  local -a mode=()
  while [ "$1" != -- ]; do mode+=("$1"); shift; done
  shift
  printf '%s' "$LOAD" >"$DIFF_TMP/loadavg"
  diff_run timeout -k 2 150 unshare -mUr sh -c '
    if [ -n "$1" ]; then mount --bind "$2" /proc || exit 125
    else mount --bind "$3" /proc/loadavg || exit 125; fi
    shift 3
    exec "$@"' _ "$NOPROC" "$DIFF_TMP/emptydir" "$DIFF_TMP/loadavg" \
    python3 "$helper" "${mode[@]}" -- "$bindir/$s/tload" "$@" \
    >"$DIFF_TMP/$s.out" 2>"$DIFF_TMP/$s.err"
  printf '%s\n' "$?" >"$DIFF_TMP/$s.rc"
}

# plain SIDE ARGS...: tload run as `tload`, for a run that ends by itself.
# OUTTO says where standard output goes.
OUTTO='file'
plain() {
  local s=$1; shift
  printf '%s' "$LOAD" >"$DIFF_TMP/loadavg"
  : >"$DIFF_TMP/$s.out"
  # bash, not sh: `exec -a` and `PIPESTATUS` are bash's.
  diff_run timeout -k 2 30 unshare -mUr bash -c '
    mount --bind "$1" /proc/loadavg || exit 125
    out=$2 outto=$3 bin=$4
    shift 4
    case $outto in
      full) exec >/dev/full ;;
      closed) exec >&- ;;
      epipe)
        trap "" PIPE
        { sleep 0.3; exec -a tload "$bin" "$@"; } | true
        exit "${PIPESTATUS[0]}"
        ;;
      *) exec >"$out" ;;
    esac
    exec -a tload "$bin" "$@"' _ "$DIFF_TMP/loadavg" "$DIFF_TMP/$s.out" "$OUTTO" \
    "$bindir/$s/tload" "$@" 2>"$DIFF_TMP/$s.err"
  printf '%s\n' "$?" >"$DIFF_TMP/$s.rc"
}

same() {
  local f
  for f in out err rc; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local s
  for s in ours gnu; do
    printf -- '--- %s: status %s\n' "$s" "$(cat "$DIFF_TMP/$s.rc")"
    printf 'out (%s bytes):\n' "$(wc -c <"$DIFF_TMP/$s.out")"
    od -An -c "$DIFF_TMP/$s.out" | head -12
    printf 'err:\n'; head -5 "$DIFF_TMP/$s.err"
  done
}

judge() {
  local label=$1 want=$2 why=${3:-}
  if same; then
    if [ "$want" = differ ]; then
      xpass=$((xpass+1))
      printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
    else
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
    fi
  elif [ "$want" = differ ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
  fi
  return 0
}

# frames LABEL MODE... -- ARGS...
frames() {
  local label=$1; shift
  side ours "$@"
  side gnu "$@"
  judge "$label" same
}

# xframes WHY LABEL MODE... -- ARGS...: frames that differ on purpose.
xframes() {
  local why=$1 label=$2; shift 2
  side ours "$@"
  side gnu "$@"
  judge "$label" differ "$why"
}

# run LABEL ARGS... / xrun WHY LABEL ARGS...
run() {
  local label=$1; shift
  plain ours "$@"
  plain gnu "$@"
  judge "$label" same
}
xrun() {
  local why=$1 label=$2; shift 2
  plain ours "$@"
  plain gnu "$@"
  judge "$label" differ "$why"
}

# --- one frame, 80 by 25, for every kind of load file --------------------------
for load in '0.50 0.40 0.30 1/100 1234' '0.00 0.00 0.00 1/1 1' \
            '2.25 1.50 0.75 3/300 9' '24.99 10 5 1/1 1' '25 10 5 1/1 1' \
            '30.00 20.00 10.00 1/1 1' '1000 500 250 1/1 1' '1e30 1e20 1e10' \
            '1e300 1 1' '1 inf nan' '-1.5 -2 -3' '0.005 0.015 0.025' \
            '0.004999 1.005 2.675' '0x1p-2 0x10 1e-5' '  7.5   8.5   9.5' \
            '1.5 2.5' '1.5' 'abc' ''; do
  LOAD=$load
  frames "one frame of '$load'" pipe 1 --
done
# A height that is not a finite number: upstream spins without drawing, and
# is killed at the deadline; this draws the column empty.
for load in 'inf inf inf' 'nan -nan nan' '-inf 1 1'; do
  LOAD=$load
  xframes "upstream spins" "one frame of '$load'" --deadline 3 pipe 1 --
done
reset_knobs
NOPROC=1
frames "no /proc/loadavg at all" pipe 1 --
reset_knobs

# --- the scale ---------------------------------------------------------------------
for s in 1 2.5 0 1e-300 100 0.001 25 1e300 0x8 -0; do
  LOAD='3.5 2 1 1/1 1'
  frames "-s $s" pipe 1 -- -s "$s"
done
for s in inf nan 1e308; do
  LOAD='3.5 2 1 1/1 1'
  xframes "upstream spins" "-s $s" --deadline 3 pipe 1 -- -s "$s"
done
LOAD='0 0 0 1/1 1'
xframes "upstream spins" "-s inf with no load" --deadline 3 pipe 1 -- -s inf
reset_knobs

# --- frames over time ----------------------------------------------------------------
LOAD='3.5 2 1 1/1 1'
frames "three frames, a second apart" pipe 3 -- -d 1
LOAD='40 20 10 1/1 1'
frames "three frames, the scale halved and drifting back" pipe 3 -- -d 1
LOAD='40 20 10 1/1 1'
frames "three frames at -s 50" pipe 3 -- -d 1 -s 50
reset_knobs

# --- other screens -------------------------------------------------------------------
frames "a 5 by 12 terminal" pty 5 12 1 --
frames "a 40 by 200 terminal" pty 40 200 1 --
frames "a 2 by 2 terminal, the figures longer than it" pty 2 2 2 -- -d 1
# Eight rows of six: the figures (17 bytes) cover the first three rows and
# the graph shows below them.
LOAD='2 1 0.5 1/1 1'
frames "an 8 by 6 terminal past its width" pty 8 6 9 -- -d 1
LOAD='9 1 0.5 1/1 1'
frames "an 8 by 6 terminal, the scale halved" pty 8 6 4 -- -d 1
LOAD='2 1 0.5 1/1 1'
frames "resized from 8 by 6 to 10 by 7 after three frames" pty 8 6 6 3 10 7 -- -d 1
frames "resized to the same size" pty 8 6 4 2 8 6 -- -d 1
frames "a 1 by 5 terminal" pty 1 5 1 --
frames "a 5 by 1 terminal" pty 5 1 1 --
reset_knobs

# --- options and refusals -----------------------------------------------------------------
run "--help" --help
run "-h" -h
xrun "names SlateOS" "-V" -V
xrun "names SlateOS" "--version" --version
for a in -z --bogus -s --scale --delay --help=x --sc; do
  run "$a alone" "$a"
done
for a in -1 abc 1e999 1x; do
  run "-s $a" -s "$a"
done
for a in 0 -1 abc 4294967296 99999999999999999999 1.5; do
  run "-d $a" -d "$a"
done
xrun "no stale errno" "-s ''" -s ''
xrun "no stale errno" "-d ''" -d ''
run "a terminal that is not there" /nonexistent/tty
run "a directory as the terminal" /

# --- where the frames go when it is standard output ------------------------------------
OUTTO='full'
run "stdout full"
OUTTO='closed'
run "stdout closed"
OUTTO='epipe'
run "reader gone, SIGPIPE ignored"
OUTTO='file'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

#!/usr/bin/env bash
# Differential test: our `kill` against procps-ng 4.0.4's, which is Ubuntu's
# `/usr/bin/kill` (procps-ref.sh builds its procps with `--disable-kill`, so
# the reference is the installed one).
#
# ## It can signal nothing but its own processes
#
# Every case runs each side in its own `unshare -Urmpf --mount-proc`: a user
# namespace in which the harness is root, and a PID namespace in which the
# shell that runs the case is process 1. `kill(2)` there names only the
# namespace's processes, and `-1` -- "every process I may signal" -- is every
# one of them but process 1 and the caller. So no case, on either side, can
# reach anything outside, which is what lets this harness measure upstream's
# worst defect at all: `kill -9 -1234` reads `-1234` as the options `-1`,
# `-2`..., and sends `SIGKILL` to `'0' - '1'`, which is -1. In the namespace
# that kills the case's own victims and nothing else.
#
# A case's processes are two victims, `sleep`s each in a process group of its
# own, started before the program runs. A case names them with placeholders,
# replaced inside the namespace:
#
#   @V  the first victim's PID     @W  the second's
#   @G  -PGID of the first victim's group, a negative number
#   @X  4294967296 + the first victim's PID, which a `pid_t` cuts to it
#
# After the program the victims' fates are read from the namespace's own
# `/proc` -- `alive`, `stopped`, or the signal that ended it -- and compared
# with the program's output, error output and status. In a fresh PID
# namespace the victims are processes 2 and 3 on both sides, so a PID in a
# message agrees too.
#
# ## Cases that differ on purpose
#
# The module's deliberate differences: a negative PID that is not after `--`
# is that process group, where upstream signals -1 and inverts its status;
# `-l SIG9` is an unknown name, where upstream aborts on a bad `free`; `-V`
# names this build; a failed write to standard output is reported; and an
# empty number is reported with no reason, where upstream prints whatever
# `errno` its start-up left. See `userspace/coreutils/src/bin/kill.rs`.
set -u

DIFF_PROG='kill'
# `command -v kill` finds the shell's builtin, which is not what is compared.
DIFF_REF='/usr/bin/kill /bin/kill'
DIFF_NEED='unshare setsid timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if ! unshare -Urmpf --mount-proc true 2>/dev/null; then
  echo "kill-diff: cannot make a user and PID namespace here; SKIPPED"
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/kill
mkdir -p "$work"
case_no=0

# The script each side runs as process 1 of its namespace: start the victims,
# put their numbers into the arguments, run the program, then say what became
# of each victim.
nsrun=$work/nsrun.sh
cat >"$nsrun" <<'NSRUN'
bindir=$1 out=$2 err=$3 redir=$4; shift 4
setsid sleep 1000 & v=$!
setsid sleep 1000 & w=$!
# Each victim leads its own group once `setsid` has run; wait for both.
n=0
while [ "$n" -lt 100 ]; do
  pv=$(cut -d' ' -f5 "/proc/$v/stat" 2>/dev/null)
  pw=$(cut -d' ' -f5 "/proc/$w/stat" 2>/dev/null)
  [ "$pv" = "$v" ] && [ "$pw" = "$w" ] && break
  sleep 0.02; n=$((n + 1))
done
[ "$pv" = "$v" ] && [ "$pw" = "$w" ] || exit 125
# The placeholders replaced, in place: each argument taken off the front and
# put back on the end, once round, so none is split, globbed or lost --
# empty ones included.
n=$#
while [ "$n" -gt 0 ]; do
  a=$1; shift
  case $a in
    @V) a=$v ;;
    @W) a=$w ;;
    @G) a=-$v ;;
    @X) a=$((4294967296 + v)) ;;
    +@V) a=+$v ;;
  esac
  set -- "$@" "$a"
  n=$((n - 1))
done
case $redir in
  '>/dev/full') env PATH="$bindir" kill "$@" >/dev/full 2>"$err" ;;
  '>&-')        env PATH="$bindir" kill "$@" >&- 2>"$err" ;;
  *)            env PATH="$bindir" kill "$@" >"$out" 2>"$err" ;;
esac
rc=$?
# A fate. A victim gone from /proc, or a zombie there, was ended, and `wait`
# says by what: this shell is process 1, and it reaps any child that has died
# whenever it waits for something -- a `sleep` below -- so a victim killed is
# usually gone before it is looked for, never seen as a zombie. A stopped one
# is `T`. Anything else is alive, after a moment for a signal in flight. Run
# in this shell, not in a `$( )`: only the parent can `wait`.
fate() {
  i=0
  while :; do
    st=$(cut -d' ' -f3 "/proc/$1/stat" 2>/dev/null)
    case $st in
      ''|Z) wait "$1"; printf 'died %s' "$(( $? - 128 ))"; return ;;
      T|t) printf stopped; return ;;
    esac
    i=$((i + 1))
    if [ "$i" -ge 20 ]; then printf alive; return; fi
    sleep 0.02
  done
}
printf 'rc=%s V:' "$rc"; fate "$v"; printf ' W:'; fate "$w"; printf '\n'
NSRUN

# Knobs for the next case, reset after it: where standard output goes.
REDIR=''

# `run_side SIDE PREFIX ARGS...`
run_side() {
  local side=$1 p=$2; shift 2
  : >"$p.$side.out"; : >"$p.$side.err"
  diff_run timeout -k 5 30 unshare -Urmpf --mount-proc sh "$nsrun" \
    "$bindir/$side" "$p.$side.out" "$p.$side.err" "$REDIR" "$@" \
    >"$p.$side.fate" 2>"$p.$side.nserr" </dev/null
  echo $? >"$p.$side.rc"
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="kill $*${REDIR:+ $REDIR}"
  REDIR=''
  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  if [ "$o_rc" != 0 ] || [ "$g_rc" != 0 ]; then
    AGREED=broken
    REPORT="  namespace rc ours=$o_rc gnu=$g_rc
$(head -5 "$p.ours.nserr")
$(head -5 "$p.gnu.nserr")"
    return 0
  fi
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && cmp -s "$p.ours.fate" "$p.gnu.fate"; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours %s\n%s\n  ~~~ stderr\n%s\n  --- gnu %s\n%s\n  ~~~ stderr\n%s' \
    "$(cat "$p.ours.fate")" "$(cat -A "$p.ours.out" | head -20)" "$(cat -A "$p.ours.err" | head -10)" \
    "$(cat "$p.gnu.fate")" "$(cat -A "$p.gnu.out" | head -20)" "$(cat -A "$p.gnu.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- the namespace did not run the case\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = broken ]; then
    report
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$LABEL" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n%s\n' "$LABEL" "$why" "$REPORT"
  fi
  return 0
}

# --- no arguments, help, version ---------------------------------------------
run_case
run_case -h
run_case --help
run_case --he
xfail_case 'our version string' -V
xfail_case 'our version string' --version

# --- signalling the victims ---------------------------------------------------
run_case @V
run_case @V @W
run_case -9 @V
run_case -KILL @V
run_case -kill @V
run_case -SIGKILL @V
run_case -sigint @V
run_case -1 @V
run_case -HUP @V @W
run_case -s HUP @V
run_case -s 1 @V
run_case -sHUP @V
run_case -s sighup @V
run_case --signal USR1 @V
run_case --signal=USR1 @V
run_case --sig=USR2 @V
run_case -s CLD @V
run_case -s IO @V
run_case -s IOT @V
run_case -STOP @V
run_case -s STOP @V
run_case -CONT @V
run_case -s 0 @V
run_case -0 @V
run_case -s NULL @V
run_case -s EXIT @V
run_case -RTMIN @V
run_case -RTMIN+1 @V
run_case -s RTMIN+2 @V
run_case -s 64 @V
run_case -s 93 @V
run_case -s 94 @V
# The signal word anywhere, even after the PIDs and after `--`.
run_case @V -9
run_case @V -TERM @W
run_case -- @V -9
run_case -9 -HUP @V
run_case @V -s 9
# Names that are no signal: `-s` gives -1, which kill(2) refuses per process.
run_case -s FOO @V
run_case -s '' @V
run_case -s SIG @V
run_case -s 9x @V
# --- PIDs -----------------------------------------------------------------------
run_case 999999
run_case @V 999999 @W
run_case @X
run_case abc
run_case @V abc @W
why_empty='an empty number is reported with no reason; upstream prints a stale errno (deliberate difference 5)'
xfail_case "$why_empty" ''
run_case ' 3'
run_case +@V
run_case 99999999999999999999
run_case 0x10
run_case -- @V
run_case -- @V @W
# Process groups after `--`, and every process: with the null signal, which
# sends nothing, and then for real -- every process here is the case's own.
run_case -0 -- @G
run_case -s 0 -- @G
run_case -TERM -- @G
run_case -9 -- -1
run_case -0 -- -1
run_case -0 -- -999999
run_case -0 -- 0
# A negative PID with no `--`: upstream signals -1 -- every process here --
# with its status inverted; ours, the group named.
why_neg='a negative PID is its process group, not -1 (deliberate difference 1)'
xfail_case "$why_neg" -0 @G
xfail_case "$why_neg" -9 @G
xfail_case "$why_neg" @V -0 @G
# A failure inverted: no group 5, which upstream reports as success.
xfail_case "$why_neg" -0 -5
# Two digits: upstream reads only the first. `-23` is the group 2 there.
xfail_case "$why_neg" -0 -23
# And the defect itself: `-12` is -1 upstream, and SIGKILL goes to every
# process the caller may signal -- here, the two victims and nothing more.
xfail_case "$why_neg" -9 -12
# --- sigqueue -------------------------------------------------------------------
run_case -q 5 @V
run_case -q 5 -s USR1 @V
run_case --queue=7 @V
run_case -q abc @V
xfail_case "$why_empty" -q '' @V
run_case -q 4294967297 @V
run_case -q 99999999999999999999 @V
run_case -q 5 999999
# --- -l and -L ------------------------------------------------------------------
run_case -l
run_case -L
run_case --list
run_case --table
run_case --tab
run_case -l 9
run_case -l 15
run_case -l 0
run_case -l 31
run_case -l 32
run_case -l 4294967305
run_case -l KILL
run_case -l kill
run_case -l SIGKILL
run_case -l sigterm
run_case -l POLL
run_case -l IO
run_case -l CLD
run_case -l RTMIN
run_case -l nosuch
run_case -l ''
run_case -l ' 9'
run_case -l 9x
run_case -lTERM
run_case -l9
run_case --list=TERM
run_case --list=9
run_case --list TERM
run_case -l -- TERM
run_case -l TERM @V
run_case @V -l TERM
run_case @V -l
run_case -L @V
xfail_case 'SIG9 is an unknown name; upstream aborts on a bad free (deliberate difference 2)' -l SIG9
# --- refusals -------------------------------------------------------------------
run_case -Z @V
run_case -x
run_case --nosuch @V
run_case --signal
run_case -s
run_case -q
run_case --queue
run_case --table=x
run_case --help=x
run_case -9
run_case -s 9
run_case --
run_case -- -9
# --- output that cannot be written ---------------------------------------------
why_write='a failed write to standard output is reported (deliberate difference 4)'
REDIR='>/dev/full'; xfail_case "$why_write" -l
REDIR='>/dev/full'; xfail_case "$why_write" -L
REDIR='>&-';        xfail_case "$why_write" -l 9
REDIR='>/dev/full'; xfail_case "$why_write" --help
REDIR='>/dev/full'; run_case @V
REDIR='>&-';        run_case -9 @V
REDIR='>/dev/full'; run_case -l nosuch

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
if [ "$broken" -gt 0 ]; then
  printf ', %d BROKEN' "$broken"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$broken" = 0 ]

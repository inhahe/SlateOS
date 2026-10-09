#!/usr/bin/env bash
# Differential test: our `killall` against psmisc 23.7's, Ubuntu's
# `/usr/bin/killall`.
#
# ## It can signal nothing but its own processes
#
# As in `kill-diff.sh`: every case runs each side in its own
# `unshare -Urmpf --mount-proc`, a user namespace in which the harness is root
# and a PID namespace in which the shell running the case is process 1, so
# `killall`'s `/proc` holds the case's processes and nothing else. Whatever a
# case asks for -- `-r '.*'`, `-g`, `-u root` -- the processes it can reach
# are the case's own.
#
# A case's processes are four victims, each in a process group of its own,
# started before the program runs: copies of `sleep` named
#
#   V  victim                  (process 2)
#   W  victim                  (process 3), the same name twice
#   L  averyveryverylongname   (process 4), whose name the kernel cuts to
#                              15 bytes, so `killall` must read `cmdline`
#   S  sleep                   (process 5)
#
# and a NAME of `@BIN/victim` is the victims' own file, for the cases that
# name a process by its executable. After the program the victims' fates are
# read from the namespace's own `/proc` -- `alive`, `stopped`, or the signal
# that ended it -- and compared with the program's output, error output and
# status. `INPUT` is what `-i` reads from standard input.
#
# ## Cases that differ on purpose
#
# The module's deliberate differences: `-V` names this build; a failed write
# to standard output is reported. See `userspace/coreutils/src/bin/killall.rs`.
set -u

DIFF_PROG='killall'
DIFF_REF='/usr/bin/killall /bin/killall'
DIFF_NEED='unshare setsid timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if ! unshare -Urmpf --mount-proc true 2>/dev/null; then
  echo "killall-diff: cannot make a user and PID namespace here; SKIPPED"
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/killall
fx=$work/bin
mkdir -p "$fx"
case_no=0
sleep_bin=$(command -v sleep)
cp "$sleep_bin" "$fx/victim"
cp "$sleep_bin" "$fx/averyveryverylongname"

# Each side's script, run as process 1 of its namespace.
nsrun=$work/nsrun.sh
cat >"$nsrun" <<'NSRUN'
bindir=$1 fx=$2 sleep_bin=$3 out=$4 err=$5 input=$6 redir=$7; shift 7
setsid "$fx/victim" 1000 & v=$!
setsid "$fx/victim" 1000 & w=$!
setsid "$fx/averyveryverylongname" 1000 & l=$!
setsid "$sleep_bin" 1000 & s=$!
n=0
while [ "$n" -lt 100 ]; do
  ok=yes
  for p in "$v" "$w" "$l" "$s"; do
    [ "$(cut -d' ' -f5 "/proc/$p/stat" 2>/dev/null)" = "$p" ] || ok=no
  done
  [ "$ok" = yes ] && break
  sleep 0.02; n=$((n + 1))
done
[ "$ok" = yes ] || exit 125
# `@BIN` in an argument is the victims' directory.
n=$#
while [ "$n" -gt 0 ]; do
  a=$1; shift
  case $a in
    @BIN/*) a=$fx/${a#@BIN/} ;;
  esac
  set -- "$@" "$a"
  n=$((n - 1))
done
case $redir in
  '>/dev/full') printf '%b' "$input" | env PATH="$bindir" killall "$@" >/dev/full 2>"$err" ;;
  '>&-')        printf '%b' "$input" | env PATH="$bindir" killall "$@" >&- 2>"$err" ;;
  *)            printf '%b' "$input" | env PATH="$bindir" killall "$@" >"$out" 2>"$err" ;;
esac
rc=$?
# A fate, as kill-diff.sh reads one: gone or a zombie was ended, `T` is
# stopped, anything else alive after a moment for a signal in flight. In this
# shell, not a `$( )`, since only the parent can `wait`.
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
printf 'rc=%s V:' "$rc"; fate "$v"; printf ' W:'; fate "$w"
printf ' L:'; fate "$l"; printf ' S:'; fate "$s"; printf '\n'
NSRUN

# Knobs for the next case, reset after it.
REDIR=''
INPUT=''

# `run_side SIDE PREFIX ARGS...`
run_side() {
  local side=$1 p=$2; shift 2
  : >"$p.$side.out"; : >"$p.$side.err"
  diff_run timeout -k 5 40 unshare -Urmpf --mount-proc sh "$nsrun" \
    "$bindir/$side" "$fx" "$sleep_bin" "$p.$side.out" "$p.$side.err" "$INPUT" "$REDIR" "$@" \
    >"$p.$side.fate" 2>"$p.$side.nserr" </dev/null
  echo $? >"$p.$side.rc"
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="killall $*${REDIR:+ $REDIR}${INPUT:+ <<< $INPUT}"
  REDIR=''; INPUT=''
  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  if [ "$o_rc" != 0 ] || [ "$g_rc" != 0 ]; then
    AGREED=broken
    REPORT="  namespace rc ours=$o_rc gnu=$g_rc
$(head -5 "$p.ours.nserr")
$(head -5 "$p.gnu.nserr")"
    return 0
  fi
  # The victims' directory is a temporary one, the same on both sides;
  # shown as @BIN in a message, so a report reads the same from run to run.
  local f
  for f in "$p.ours.out" "$p.gnu.out" "$p.ours.err" "$p.gnu.err"; do
    sed -i "s|$fx|@BIN|g" "$f"
  done
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && cmp -s "$p.ours.fate" "$p.gnu.fate"; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours %s\n%s\n  ~~~ stderr\n%s\n  --- gnu %s\n%s\n  ~~~ stderr\n%s' \
    "$(cat "$p.ours.fate")" "$(cat -A "$p.ours.out" | head -20)" "$(cat -A "$p.ours.err" | head -12)" \
    "$(cat "$p.gnu.fate")" "$(cat -A "$p.gnu.out" | head -20)" "$(cat -A "$p.gnu.err" | head -12)")
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

# --- usage, version, the list --------------------------------------------------
run_case
run_case -e
run_case -q
run_case --
run_case -x victim
run_case -h
run_case --help
xfail_case 'our version string' -V
xfail_case 'our version string' --version
run_case -l
run_case --list-signals
run_case -list
run_case -l victim

# --- names ---------------------------------------------------------------------
run_case victim
run_case sleep
run_case victim sleep
run_case nosuch
run_case victim nosuch
run_case -q nosuch
run_case -q victim nosuch
run_case averyveryverylongname
run_case averyveryverylo
run_case averyveryverylongnameX
run_case -e averyveryverylongname
run_case -v -e averyveryverylo
run_case VICTIM
run_case -I VICTIM
run_case --ignore-case Victim
run_case @BIN/victim
run_case @BIN/averyveryverylongname
run_case /nonexistent/victim
run_case ./victim
run_case -- -victim

# --- signals -------------------------------------------------------------------
run_case -s HUP victim
run_case -s 1 victim
run_case -s SIGHUP victim
run_case -s hup victim
run_case -s 9x victim
run_case -s 0 victim
run_case -s STOP sleep
run_case --signal=USR1 victim
run_case -HUP victim
run_case -SIGKILL victim
run_case -9 victim
run_case -15 sleep
run_case -INT victim
run_case -IOT victim
run_case -VTALRM victim
run_case -KILL -HUP victim
run_case -ZZ victim
run_case victim -9

# --- regular expressions --------------------------------------------------------
run_case -r '^vic'
run_case -r 'tim$'
run_case -r 'averyveryverylongname'
run_case -r '^averyveryverylo$'
run_case -r -I '^VIC'
run_case -r '('
run_case -r 'nomatch'
run_case --regexp victim

# --- narrowing -------------------------------------------------------------------
run_case -u root victim
run_case -u 0 victim
run_case -u nosuchuser victim
run_case -u root
run_case -y 1h victim
run_case -o 1h victim
run_case -o 1s -y 1h victim
run_case -y 10 victim
run_case -y 0s victim
run_case -o x victim
run_case -y 2M victim
run_case -n 1 victim
run_case -n 999999 victim
run_case -n x victim
run_case -n 1x victim
run_case -Z . victim
run_case -Z '[' victim
run_case -Z nomatch victim
run_case -Z .

# --- groups, waiting, verbosity, asking ------------------------------------------
run_case -g victim
run_case -g -v victim
run_case -v victim
run_case -v -s 0 sleep
run_case -ve victim
run_case -w victim
# Not `-w -s 0`: a signal that ends nothing is waited out forever, on both
# sides, until the case's timeout kills the namespace.
run_case -w -s HUP victim
INPUT='y\nn\n';   run_case -i victim
INPUT='n\ny\n';   run_case -i victim
INPUT='x\ny\n\n'; run_case -i victim
INPUT='';         run_case -i victim
INPUT='y\n';      run_case -i -g sleep
INPUT='Y\nN\n';   run_case -i -s HUP victim

# --- output that cannot be written ---------------------------------------------
why_write='a failed write to standard output is reported (deliberate difference 4)'
REDIR='>/dev/full'; xfail_case "$why_write" -l
REDIR='>&-';        xfail_case "$why_write" -l
REDIR='>/dev/full'; INPUT='y\ny\n'; xfail_case "$why_write" -i victim
REDIR='>/dev/full'; run_case victim
REDIR='>&-';        run_case -v victim

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
if [ "$broken" -gt 0 ]; then
  printf ', %d BROKEN' "$broken"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$broken" = 0 ]

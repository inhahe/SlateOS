#!/bin/bash
# skill-diff.sh -- run our `skill` and `snice` and procps-ng's side by side and
# report every case where they disagree.
#
# ## The reference
#
# Ubuntu's /usr/bin/skill, procps 4.0.4-4ubuntu3.2, and /usr/bin/snice, which
# is a link to it: one program that is one or the other by its name.
#
# ## What it may touch
#
# These programs signal and renice processes picked by name, user, terminal
# or number, across the whole system. So every case is safe by construction:
# it either signals nothing (`-n`, which sends signal 0, or a name nothing
# has), sends signal 0, STOP and CONT, or renices -- upwards only, which any
# user may -- processes this harness started, each under a name no other
# program has: copies of `sleep` called `skilltgt`, `skilltgt2` and
# `snicetgt`. They are killed by their pids when the harness exits.
#
# ## The knobs, reset before each group
#
#   ARGV0  argv[0]: the name decides the program, so it is set every case.
#   STDIN  text standard input holds, for `-i`'s questions.
#   ERRTO  `merge` puts standard error into standard output, to compare the
#          two in order; `full` and `closed` as they say.
#   OUTTO  `full`, `closed`, `epipe` (the reader gone, SIGPIPE ignored).
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS; an argument echoed between apostrophes is
# quoted as `quoteaf` quotes it, which differs only for one holding an
# apostrophe or something unprintable.
#
# Run `OURS=/usr/bin ./scripts/skill-diff.sh` to check that the harness still
# discriminates: every case that differs on purpose should then be reported
# as no longer differing, and nothing else.
set -u

DIFF_PROG='skill'
DIFF_BINS='skill snice'
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- the targets --------------------------------------------------------------
pids=()
diff_cleanup() {
  [ "${#pids[@]}" -gt 0 ] && kill "${pids[@]}" 2>/dev/null
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
  return 0
}
mkdir -p "$DIFF_TMP/t"
for n in skilltgt skilltgt2 snicetgt; do
  cp "$(command -v sleep)" "$DIFF_TMP/t/$n"
done
"$DIFF_TMP/t/skilltgt" 600 & t1=$!; pids+=("$t1")
"$DIFF_TMP/t/skilltgt" 600 & t2=$!; pids+=("$t2")
"$DIFF_TMP/t/skilltgt2" 600 & t3=$!; pids+=("$t3")
"$DIFF_TMP/t/snicetgt" 600 & t4=$!; pids+=("$t4")
# Each has become its program before it is asked about.
for p in "$t1" "$t2" "$t3" "$t4"; do
  n=0
  until grep -q 'tgt' "/proc/$p/comm" 2>/dev/null; do
    n=$((n+1)); [ "$n" -gt 300 ] && { echo "skill-diff: target $p never started" >&2; exit 1; }
    sleep 0.01
  done
done
me=$(id -un)
p_none=2147483646

# --- one run ------------------------------------------------------------------
cat >"$DIFF_TMP/run1" <<'EOF'
#!/bin/bash
# run1 OUTTO ERRTO OUT ERR IN ARGV0 BIN ARGS...
outto=$1 errto=$2 out=$3 err=$4 in=$5 a0=$6 bin=$7
shift 7
exec <"$in"
case $outto in
  file) exec >"$out" ;;
  full) exec >/dev/full ;;
  closed) exec >&- ;;
esac
case $errto in
  file) exec 2>"$err" ;;
  merge) exec 2>&1 ;;
  full) exec 2>/dev/full ;;
  closed) exec 2>&- ;;
esac
case $outto in
  epipe)
    trap '' PIPE
    { sleep 0.3; exec -a "$a0" "$bin" "$@"; } | true
    exit "${PIPESTATUS[0]}"
    ;;
esac
exec -a "$a0" "$bin" "$@"
EOF
chmod +x "$DIFF_TMP/run1"

OUTTO='file'; ERRTO='file'; STDIN=
reset_knobs() { OUTTO='file'; ERRTO='file'; STDIN=; }

# run_side SIDE PROG ARGV0 ARGS...
run_side() {
  local side=$1 prog=$2 a0=$3; shift 3
  printf '%s' "$STDIN" >"$DIFF_TMP/in"
  : >"$DIFF_TMP/$side.out"; : >"$DIFF_TMP/$side.err"
  diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 "$DIFF_TMP/run1" "$OUTTO" "$ERRTO" \
    "$DIFF_TMP/$side.out" "$DIFF_TMP/$side.err" "$DIFF_TMP/in" "$a0" \
    "$bindir/$side/$prog" "$@"
  printf '%s\n' "$?" >"$DIFF_TMP/$side.rc"
}

same() {
  local f
  for f in out err rc; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local side
  for side in ours gnu; do
    printf -- '--- %s: status %s\n' "$side" "$(cat "$DIFF_TMP/$side.rc")"
    printf 'out:\n'; cat -A "$DIFF_TMP/$side.out" | head -20
    printf 'err:\n'; cat -A "$DIFF_TMP/$side.err" | head -20
  done
}

# check LABEL PROG ARGV0 ARGS...
check() {
  local label=$1; shift
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
  fi
  return 0
}

xcheck() {
  local why=$1 label=$2; shift 2
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  fi
  return 0
}

sk() { local label=$1; shift; check "skill: $label" skill skill "$@"; }
sn() { local label=$1; shift; check "snice: $label" snice snice "$@"; }

# --- picking processes, signalling none ------------------------------------------
sk "-n by command" -n -c skilltgt
sk "-n by an operand that is a name" -n skilltgt
sk "-n by two names" -n skilltgt skilltgt2
sk "-n by pid" -n -p "$t1"
sk "-n by an operand that is a number" -n "$t1" "$t3"
sk "-n a pid nothing has" -n "$p_none"
sk "-n by user and command" -n -u "$me" -c skilltgt
sk "-n a user nobody is, with a command" -n -u nosuchuser -c skilltgt
sk "-n a terminal nothing uses" -n -t null -c skilltgt
sk "-n a terminal that is not there" -n -t nosuchtty -c skilltgt
sk "-n a terminal named past 31 bytes" -n -t "$(printf 'x%.0s' $(seq 40))" -c skilltgt
sk "-n by --ns alone and a command" -n --ns "$$" -c skilltgt
sk "-n by --ns and --nslist" -n --ns "$$" --nslist pid,mnt -c skilltgt
sk "-n a command nothing has" -n -c nosuchcommand
sk "-n an operand with a space" -n " $t1"
sn "-n by command" -n -c skilltgt
sn "-n with a priority" +7 -n -c skilltgt

# --- signals that do nothing, said aloud ------------------------------------------
sk "signal 0, verbose" -0 -v -c skilltgt
sk "signal NULL, verbose" -NULL -v -c skilltgt
sk "signal 0, debug" -0 -d -c skilltgt
sk "signal 0, warnings, no failure" -0 -w -c skilltgt
sk "signal 0, warnings, init" -0 -w -p 1
sk "signal 0, verbose, init" -0 -v 1
sk "STOP, verbose" -STOP -v -c skilltgt2
sk "CONT, verbose" -SIGCONT -v -c skilltgt2
sk "cont in lower case" -cont -v -c skilltgt2
sk "18 is CONT" -18 -v -c skilltgt2
sk "the signal after other words" -v -c skilltgt2 -CONT
sk "two signals: the first is taken" -CONT -STOP -v -c skilltgt2
sk "CONT again" -CONT -c skilltgt2
for s in RTMIN RTMIN+1 IO IOT CLD 64 SIGHUP sigterm; do
  sk "-d with -$s and nothing picked" -d "-$s" -c nosuchcommand
done
sk "-100 is not a signal" -100 -c nosuchcommand
sk "-n overrides the signal" -n -KILL -d -c nosuchcommand

# --- snice, on its own target ------------------------------------------------------
sn "+5, verbose" +5 -v -c snicetgt
sn "+0 after +5 is refused" +0 -v -c snicetgt
sn "-5 is refused" -5 -v -c snicetgt
sn "+20 is kept to 19" +20 -v -c snicetgt
sn "the last priority wins" +3 +6 -v -c snicetgt
sn "the default, +4" -v -c snicetgt
sn "a priority after the expression" -v -c snicetgt +8
sn "+3x" +3x -c snicetgt
sn "a priority out of int range" +99999999999 -c snicetgt
sn "a negative one out of range" -99999999999 -c snicetgt
sn "-w with a refusal" -1 -w -c snicetgt
sn "-d" +9 -d -c snicetgt

# --- -i: questions on standard error, answers from standard input -----------------
STDIN=$'y\nn\n'
sk "-i yes and no" -0 -i -c skilltgt
STDIN=$'Yes\n'
sk "-i with one answer for two" -0 -i -c skilltgt
STDIN=
sk "-i with no answers" -0 -i -c skilltgt
reset_knobs

# --- listings, help, version ---------------------------------------------------------
for o in -l -L --list --table --lis -lL; do
  sk "$o" "$o"
done
sn "-l" -l
for o in -h --help; do
  sk "$o" "$o"
  sn "$o" "$o"
done
xcheck "names SlateOS" "skill -V" skill skill -V
xcheck "names SlateOS" "snice --version" snice snice --version
sk "no arguments at all"
sn "no arguments at all"
sk "a signal and nothing else" -9
sn "a priority and nothing else" +5

# --- refusals -------------------------------------------------------------------------
sk "no criteria" -n
sk "no criteria, but -t and -u of nothing" -n -t nosuchtty -u nosuchuser
sk "-p abc" -n -p abc
sk "-p ''" -n -p ''
sk "-p out of range" -n -p 99999999999999999999
xcheck "quoted as quoteaf quotes" "skill: -p with an apostrophe" skill skill -n -p "1'x"
sk "-i and -v" -0 -i -v -c skilltgt
sk "-i and -n" -0 -i -n -c skilltgt
sk "-i and -f" -0 -i -f -c skilltgt
sk "-v and -f" -0 -v -f -c skilltgt
sk "-f and -n" -0 -f -n -c skilltgt
for o in -z --bogus -c --pid --n --ns --nslist --help=x; do
  sk "$o alone" "$o"
done
sk "--ns 0" -n --ns 0 -c skilltgt
sk "--ns abc" -n --ns abc -c skilltgt
sk "--ns -5" -n --ns -5 -c skilltgt
sk "--ns of a process that is not there" -n --ns "$p_none" -c skilltgt
sk "--nslist bogus, with ipc" -n --nslist ipc,bogus -c skilltgt
sk "--nslist bogus alone" -n --nslist bogus -c skilltgt
sk "--nslist bogus and --ns" -n --ns "$$" --nslist bogus -c skilltgt
sk "--nslist with an empty name" -n --nslist ipc, -c skilltgt
sk "--nslist alone" -n --nslist ipc -c skilltgt

# --- argv[0] --------------------------------------------------------------------------
check "lt-skill" skill lt-skill -n -c skilltgt
check "lt-snice" snice lt-snice -n -c skilltgt
check "a full path" skill /usr/local/bin/skill -n -c skilltgt
check "another name" skill prockill -n -c skilltgt
check "an empty name" skill '' -n -c skilltgt
check "snice's binary as skill" snice skill -0 -v -c skilltgt
check "an unknown option, by a full path" skill /x/skill -z

# --- order, and where the output goes ---------------------------------------------------
ERRTO='merge'
sk "-n and -v lines in order" -0 -v -c skilltgt
sk "-n and a refusal in order" -n -p 1 -w -c skilltgt
reset_knobs
for o in full closed epipe; do
  OUTTO=$o
  sk "stdout $o: -n" -n -c skilltgt
  sk "stdout $o: -l" -l
  sk "stdout $o: --help" --help
done
reset_knobs
ERRTO='full'
sk "stderr full: -v" -0 -v -c skilltgt
sk "stderr full: a refusal" -n
ERRTO='closed'
sk "stderr closed: -v" -0 -v -c skilltgt
reset_knobs

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]

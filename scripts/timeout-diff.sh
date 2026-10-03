#!/usr/bin/env bash
# Differential test: our `timeout` against GNU coreutils 9.4's.
#
# ## What this program's answer consists of
#
# Four things, and the last two are where a port goes wrong silently:
#
#   1. What it printed: its diagnostics, `-v`'s lines, `--help`.
#   2. What it exited with: 124 on a timeout, 125 for its own failures,
#      126/127 for a command it could not run, the command's status otherwise.
#   3. **Whether it exited at all, or died of a signal.** A shell reports both
#      as 128+N, so `$?` cannot tell them apart -- but upstream deliberately
#      kills itself with the signal its command died of, so that its own
#      parent sees a signal death too. Every case here therefore runs under
#      `ended.py`, below, which reads the wait status itself and records
#      `exit N` or `signal N`.
#   4. **What happened to the processes around it**: whether the command's
#      own children were killed with it (they are, by the process-group kill,
#      unless `--foreground`), whether the command ran in a group of its own,
#      and which signals the command started with ignored and blocked.
#
# ## Why both sides run inside WSL
#
# As for every harness built on `diff-wsl.sh`: ours is a unix binary, and
# this one is made of `fork`, `setitimer`, `sigsuspend` and `kill (0, ...)`,
# none of which the Windows host has.
#
# ## Timing
#
# The cases that time out use 0.3 seconds against commands that would run for
# 5 or 10, so a loaded machine changes how long a run takes and not what it
# answers. A case that should finish first runs a command that finishes at
# once. Each run is bounded by the system's own `timeout` at 60 seconds, which
# a case reaching would record as a difference.
#
# ## Cases that differ on purpose
#
# The family's two: `--help` omits the GNU project's link block, and
# `--version` names SlateOS.
#
# Run `OURS=$(command -v timeout) ./scripts/timeout-diff.sh` to confirm the
# harness still discriminates: it should report every xfail as XPASS and
# nothing else.
set -u

DIFF_PROG='timeout'
# Built from source, not Ubuntu's patched package: see `diff-wsl.sh`'s "Why a
# built reference" and `design-decisions.md` 726.
DIFF_GNU_SOURCE=9.4
DIFF_NEED="python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# The bound on each run: the system's `timeout`, by its full path, because
# `timeout` on PATH is the program under test.
BOUND=/usr/bin/timeout
[ -x "$BOUND" ] || { echo "timeout-diff: no $BOUND to bound the runs with; skipping"; exit 0; }

# ---------------------------------------------------------------------------
# ended.py: run a command and record how it ended
# ---------------------------------------------------------------------------
# `exit N` or `signal N`, from the wait status itself. Python ignores SIGPIPE
# and SIGXFSZ at startup and a shell does not, so the child puts both back
# before exec -- otherwise every `timeout` here would start with SIGPIPE
# ignored, which is the one disposition upstream passes on unchanged. Three
# knobs, through the environment:
#
#   ENDED_SIGPIPE=ignore   start the command with SIGPIPE ignored instead
#   ENDED_BLOCK=USR1       start it with that signal blocked
#   ENDED_SEND="TERM 0.3"  after 0.3 s, send it that signal
helper=$DIFF_TMP/ended.py
cat > "$helper" <<'PY'
import os, signal, sys, time

out, argv = sys.argv[1], sys.argv[2:]
pid = os.fork()
if pid == 0:
    signal.signal(signal.SIGXFSZ, signal.SIG_DFL)
    if os.environ.get("ENDED_SIGPIPE") == "ignore":
        signal.signal(signal.SIGPIPE, signal.SIG_IGN)
    else:
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    block = os.environ.get("ENDED_BLOCK")
    if block:
        signal.pthread_sigmask(signal.SIG_BLOCK, {signal.Signals["SIG" + block]})
    try:
        os.execvp(argv[0], argv)
    finally:
        os._exit(127)
send = os.environ.get("ENDED_SEND")
if send:
    name, delay = send.split()
    time.sleep(float(delay))
    os.kill(pid, signal.Signals["SIG" + name])
_, status = os.waitpid(pid, 0)
with open(out, "w") as f:
    if os.WIFSIGNALED(status):
        f.write("signal %d\n" % os.WTERMSIG(status))
    else:
        f.write("exit %d\n" % os.WEXITSTATUS(status))
PY

# --- knobs, reset after every case -----------------------------------------

# Compare which of help/version/other came out, not the text: for the
# abbreviation cases, whose question is which option a prefix resolves to.
KIND=
# Passed to ended.py for one case.
SIGPIPE_MODE=; BLOCK=; SEND=
reset_knobs() { KIND=; SIGPIPE_MODE=; BLOCK=; SEND=; }

classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: timeout }" != "$first" ]; then echo help
  elif [ "${first#timeout \(}" != "$first" ]; then echo version
  else echo other
  fi
}

# --- running one side --------------------------------------------------------

# `timeout ARGS`, stdin from /dev/null, stdout and stderr to separate files,
# its ending to a third. The arguments arrive untouched, so a byte that is not
# UTF-8 and a word with a space in it both reach the program as they are.
run_direct() {
  local side=$1 out=$2 err=$3 endf=$4; shift 4
  ( PATH="$bindir/$side:$PATH" ENDED_SIGPIPE=$SIGPIPE_MODE ENDED_BLOCK=$BLOCK \
      ENDED_SEND=$SEND "$BOUND" -k 5 60 \
      python3 "$helper" "$endf" timeout "$@" </dev/null >"$out" 2>"$err" ) 2>/dev/null
  local rc=$?
  [ "$rc" = 0 ] || printf 'bounded run ended %s\n' "$rc" >>"$endf"
  return 0
}

# A bash snippet, for what an argument list cannot say: a pipeline, a closed
# descriptor, a look at the processes after the run. Its own ending is what
# is recorded, so a snippet reports what it found on stdout.
run_snippet() {
  local side=$1 out=$2 err=$3 endf=$4 snippet=$5
  ( PATH="$bindir/$side:$PATH" ENDED_SIGPIPE=$SIGPIPE_MODE ENDED_BLOCK=$BLOCK \
      ENDED_SEND=$SEND WORK=$work "$BOUND" -k 5 60 \
      python3 "$helper" "$endf" bash -c "$snippet" </dev/null >"$out" 2>"$err" ) 2>/dev/null
  local rc=$?
  [ "$rc" = 0 ] || printf 'bounded run ended %s\n' "$rc" >>"$endf"
  return 0
}

# --- comparing the two sides -------------------------------------------------

judge() {
  local o_out=$1 g_out=$2 o_err=$3 g_err=$4 o_end=$5 g_end=$6 label=$7
  local o_show g_show o_e g_e o_x g_x
  if [ -n "$KIND" ]; then
    o_show="class $(classify "$o_out")"; g_show="class $(classify "$g_out")"
  else
    o_show=$(cat "$o_out"); g_show=$(cat "$g_out")
  fi
  o_e=$(cat "$o_err"); g_e=$(cat "$g_err")
  o_x=$(cat "$o_end" 2>/dev/null); g_x=$(cat "$g_end" 2>/dev/null)
  if [ "$o_show" = "$g_show" ] && [ "$o_e" = "$g_e" ] && [ "$o_x" = "$g_x" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s out{%s} err{%s}\n  gnu : %s out{%s} err{%s}' \
    "$(printf '%s' "$o_x" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_e" | tr '\n' '|')" \
    "$(printf '%s' "$g_x" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_e" | tr '\n' '|')")
  LABEL=$label
}

compare_direct() {
  case_no=$((case_no+1))
  local p=$work/$case_no
  run_direct ours "$p.oo" "$p.oe" "$p.ox" "$@"
  run_direct gnu  "$p.go" "$p.ge" "$p.gx" "$@"
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.ox" "$p.gx" \
    "timeout${SEND:+ [send $SEND]}${BLOCK:+ [block $BLOCK]}${SIGPIPE_MODE:+ [SIGPIPE $SIGPIPE_MODE]} $*"
  reset_knobs
}

compare_snippet() {
  case_no=$((case_no+1))
  local p=$work/$case_no
  run_snippet ours "$p.oo" "$p.oe" "$p.ox" "$1"
  run_snippet gnu  "$p.go" "$p.ge" "$p.gx" "$1"
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.ox" "$p.gx" \
    "[bash]${SIGPIPE_MODE:+ [SIGPIPE $SIGPIPE_MODE]} $1"
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

run_case() { compare_direct "$@"; report; }
sh_case()  { compare_snippet "$1"; report; }

# A case expected to differ, with the reason. Counted apart, so that one that
# starts agreeing is reported: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare_direct "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "timeout-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. A command that finishes in time: its status, passed through
# =============================================================================

run_case 5 true
run_case 5 false
run_case 5 sh -c 'exit 3'
run_case 5 sh -c 'exit 255'
run_case 5 printf '%s\n' hello
run_case 5 sh -c 'echo out; echo err >&2'
run_case 0 true                         # 0 is no limit
run_case 0 sleep 0.2
run_case 1e-400 sleep 0.2               # underflows to 0: no limit either
run_case -k 1 5 true
run_case --preserve-status 5 sh -c 'exit 7'

# =============================================================================
# 2. Running out of time
# =============================================================================

run_case 0.3 sleep 10
run_case -v 0.3 sleep 10
run_case --preserve-status 0.3 sleep 10    # the command's 128+15, as an exit
run_case -v --preserve-status 0.3 sleep 10
run_case ' 0x1p-2' sleep 10                # strtod's grammar: a quarter second
run_case 0.005m sleep 10                   # 0.3 seconds, as a suffix
run_case -s INT 0.3 sleep 10
run_case -s INT --preserve-status 0.3 sleep 10
run_case -v -s HUP --preserve-status 0.3 sleep 10
# A command that catches the signal and exits its own way.
run_case 0.3 sh -c 'trap "exit 7" TERM; sleep 10 & wait'
run_case --preserve-status 0.3 sh -c 'trap "exit 7" TERM; sleep 10 & wait'

# KILL reaches `timeout` too, through the group: it dies with its command.
run_case -s KILL 0.3 sleep 10
run_case -v -s KILL 0.3 sleep 10
run_case -s 9 --preserve-status 0.3 sleep 10
# ...except in the foreground, where only the command is sent it; upstream
# then forces --preserve-status so that 137 is still seen.
run_case --foreground -s KILL 0.3 sleep 10
run_case --foreground -v -s KILL 0.3 sleep 10

# -k: a command that ignores the first signal is sent KILL after the second
# interval. The `exec` keeps the foreground cases free of orphans.
run_case -k 0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case -v -k 0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case --foreground -k 0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case --foreground -v -k 0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case --preserve-status -k 0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case -v -k 0.3 -s INT 0.3 sh -c 'trap "" INT; exec sleep 10'
run_case -k 0 0.3 sleep 10                 # -k 0 is never
run_case -k -0 0.3 sleep 10                # and so is -0, by `if (kill_after)`

# Signals whose default the command does not die of: the command finishes on
# its own, and the run still timed out.
run_case -s 0 0.3 sleep 1                  # 0 sends nothing at all
run_case -v -s 0 0.3 sleep 1
run_case -v -s 256 0.3 sleep 1             # 256 folds to 0
run_case -v -s CONT 0.3 sleep 1
run_case -v -s STOP 0.3 sleep 1            # stopped, then sent CONT at once
run_case -v -s URG 0.3 sleep 1
run_case -v -s WINCH 0.3 sleep 1

# =============================================================================
# 3. The signal operand (`operand2sig`)
# =============================================================================
# `-v` prints the name the number has, which is the comparison that matters.

for s in TERM term SIGTERM SigTerm hup HUP 1 15 USR1 usr2 SIGALRM 14 PIPE 13 \
         QUIT ABRT IOT 6 SEGV 11 BUS 7 SYS POLL IO 29 PWR STKFLT 16 \
         RTMIN RTMIN+1 rtmin+15 RTMAX-14 RTMAX rtmax-1 SIGRTMIN+3 34 35 49 50 64 \
         137 143 191 300 265 007 SIG9 SIG007 sigkill Kill; do
  run_case -v -s "$s" 0.3 sleep 10
done

# Refused: no such signal, or not a whole number in range.
for s in FOO SIG sigsig SIGSIG -9 ' 9' '+9' 9x 0x9 65 200 254 255 33 32 SIG33 \
         RTMIN+31 RTMAX+1 RTMIN-1 2147483647 2147483648 99999999999 '' \
         "$(printf '\377')" "$(printf 'TE\377RM')"; do
  run_case -s "$s" 1 true
done
run_case --signal=FOO 1 true
run_case --signal FOO 1 true

# =============================================================================
# 4. The duration
# =============================================================================

for d in '' ' ' x 1x 1ss 1s2 '1 ' 1e 1e1x - + . nan NaN -1 -0.1 \
         "$(printf '1\377')" "$(printf '\377')"; do
  run_case "$d" true
done
for d in 1 1s 1m 1h 1d ' 1' +1 0x10 1e400 inf INF infinity Infinity -0 1. .5 \
         5e-1 0x.8p1; do
  run_case "$d" true
done
for d in x 1x '' -1 nan; do
  run_case -k "$d" 1 true
  run_case --kill-after="$d" 1 true
done

# =============================================================================
# 5. The command line
# =============================================================================

run_case
run_case 5
run_case x
run_case -v
run_case --
run_case -- 5
run_case -- 1 true
run_case -v -- 0.3 sleep 10
run_case 5 -v true                       # the first operand ends the options
run_case 1 true --help                   # ...so this runs `true --help`
run_case 1 printf '%s|' -v --help -s
run_case -x 1 true
run_case -9 1 true                       # no -NUM form here, unlike kill
run_case -k
run_case -s
run_case --kill-after
run_case --signal
run_case --bogus 1 true
run_case --v 1 true
run_case --ve 1 true
run_case --k=1 1 true
run_case --s=TERM 0.3 sleep 10
run_case --f 1 true
run_case --fo 1 true
run_case --p 1 true
run_case --pre 0.3 sleep 10
run_case --verb 0.3 sleep 10
run_case --foreground=x 1 true
run_case --preserve-status=x 1 true
run_case -vv 0.3 sleep 10
run_case -vk0.3 0.3 sh -c 'trap "" TERM; exec sleep 10'
run_case -sKILL 0.3 sleep 10
run_case -k0.3 -sINT 0.3 sh -c 'trap "" INT; exec sleep 10'
# Options act as they are met.
KIND=1; run_case --help -s BAD
run_case -s BAD --help
run_case -k BAD --version
KIND=1; run_case --version -k BAD
KIND=1; run_case --h
KIND=1; run_case --he
KIND=1; run_case --vers
KIND=1; run_case --help --version
KIND=1; run_case --version --help
KIND=1; run_case -v --help 1 true

xfail_case 'help omits the GNU project link block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 6. A command that cannot be run
# =============================================================================

run_case 1 /nonexistent
run_case 1 nosuchcommandanywhere
run_case 1 /etc/passwd                   # found, not executable: 126
run_case 1 /tmp                          # a directory: 126
run_case 1 ''
run_case 1 ./nope
run_case -v 0.3 /nonexistent
run_case 1 "$(printf 'na\377me')"        # the name quoted, the byte escaped
run_case 1 "$(printf 'a\nb')"
run_case 1 "$(printf '\342\200\230q\342\200\231')"   # the quotes themselves

# =============================================================================
# 7. A command killed by a signal of its own: `timeout` dies of it too
# =============================================================================
# The ending column is the point: a status of 143 would agree on `$?` while
# being an exit rather than a death.

run_case 5 sh -c 'kill -TERM $$'
run_case 5 sh -c 'kill -HUP $$'
run_case 5 sh -c 'kill -INT $$'
run_case 5 sh -c 'kill -USR1 $$'
run_case 5 sh -c 'kill -KILL $$'
run_case 5 sh -c 'kill -PIPE $$'
run_case 5 sh -c 'kill -ALRM $$'
run_case 5 sh -c 'kill -QUIT $$'          # dumps core, here, through WSL's pipe
run_case 5 sh -c 'kill -SEGV $$'
run_case 5 sh -c 'kill -ABRT $$'
run_case --preserve-status 5 sh -c 'kill -TERM $$'
run_case --foreground 5 sh -c 'kill -TERM $$'
run_case -v 5 sh -c 'kill -TERM $$'

# =============================================================================
# 8. A signal sent to `timeout` itself is passed on
# =============================================================================

SEND="TERM 0.3"; run_case 10 sleep 10
SEND="TERM 0.3"; run_case -v 10 sleep 10
SEND="TERM 0.3"; run_case --preserve-status 10 sleep 10
SEND="INT 0.3";  run_case -v 10 sleep 10
SEND="HUP 0.3";  run_case -v 10 sleep 10
SEND="QUIT 0.3"; run_case -v 10 sleep 10
SEND="TERM 0.3"; run_case --foreground -v 10 sleep 10
SEND="TERM 0.3"; run_case -s KILL -v 10 sleep 10
SEND="TERM 0.3"; run_case -k 0.3 -v 10 sh -c 'trap "" TERM; exec sleep 10'
SEND="USR1 0.3"; run_case 10 sleep 1     # not one it handles: it dies of it
SEND="ALRM 0.3"; run_case -v 10 sleep 10 # read as its own timer running out

# =============================================================================
# 9. The process group, and what the command starts with
# =============================================================================

# The command's group is `timeout`'s own -- unless --foreground.
group='p=$(awk "{print \$5}" /proc/$$/stat); [ "$p" = "$PPID" ] && echo own || echo shared'
run_case 5 sh -c "$group"
run_case --foreground 5 sh -c "$group"

# Signals ignored and blocked, as the command sees them: TTIN and TTOU reset,
# SIGPIPE as `timeout` was given it, the mask inherited.
run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
SIGPIPE_MODE=ignore; run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
BLOCK=USR1;  run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
BLOCK=TERM;  run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
BLOCK=CHLD;  run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
BLOCK=ALRM;  run_case 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
run_case --foreground 5 grep -E '^Sig(Ign|Blk)' /proc/self/status
# A TERM the caller blocked stays blocked in the command, so the timeout's
# TERM waits -- and KILL is what ends it.
BLOCK=TERM;  run_case -k 0.3 0.3 sleep 10
# timeout's own blocked ALRM is unblocked for its timer.
BLOCK=ALRM;  run_case 0.3 sleep 10
BLOCK=CHLD;  run_case 0.3 sleep 10
BLOCK=CHLD;  run_case 5 true

# The command's children: killed with it through the group, or left running
# in the foreground. The snippet reports which, then cleans up.
kids='timeout "$@" sh -c "sleep 10 & echo \$! > \"$WORK/kid\"; wait"; rc=$?
sleep 0.2
if kill -0 "$(cat "$WORK/kid")" 2>/dev/null; then echo alive; kill "$(cat "$WORK/kid")"; else echo gone; fi
echo "rc=$rc"'
sh_case "set -- 0.3; $kids"
sh_case "set -- --foreground 0.3; $kids"
sh_case "set -- -s KILL --foreground 0.3; $kids"

# =============================================================================
# 10. SIGPIPE, which upstream passes on unchanged
# =============================================================================
# With it at its default, `yes` dies of it and `timeout` dies of it after;
# ignored, `yes` sees EPIPE, says so and exits 1.

sh_case 'timeout 5 yes | head -n 1; echo "status ${PIPESTATUS[0]}"'
SIGPIPE_MODE=ignore; sh_case 'timeout 5 yes | head -n 1; echo "status ${PIPESTATUS[0]}"'

# =============================================================================
# 11. Descriptors closed or full
# =============================================================================
# A diagnostic that could not be delivered makes the status 125, whatever it
# would have been: upstream's close_stdout, with EXIT_CANCELED.

sh_case 'timeout 0.3 sleep 10 >&-; echo "status $?"'
sh_case 'timeout -v 0.3 sleep 10 2>&-; echo "status $?"'
sh_case 'timeout -v 0.3 sleep 10 2>/dev/full; echo "status $?"'
sh_case 'timeout 1 /nonexistent 2>&-; echo "status $?"'
sh_case 'timeout 1 /nonexistent 2>/dev/full; echo "status $?"'
sh_case 'timeout 2>&-; echo "status $?"'
sh_case 'timeout -s FOO 1 true 2>&-; echo "status $?"'
sh_case 'timeout --help >&-; echo "status $?"'
sh_case 'timeout --help >/dev/full; echo "status $?"'
sh_case 'timeout --version >&-; echo "status $?"'
sh_case 'timeout 1 true >&- 2>&-; echo "status $?"'
sh_case 'timeout 1 sh -c "echo hi" >&-; echo "status $?"'
sh_case 'timeout 1 sh -c "cat" <&-; echo "status $?"'
sh_case 'timeout 1 sh -c "ls /proc/self/fd/" >&-; echo "status $?"'
sh_case 'timeout 1 sh -c "ls /proc/self/fd/ >&2" 2>&1 >&-; echo "status $?"'
sh_case 'timeout 5 sh -c "kill -TERM \$\$" 2>&-; echo "status $?"'

# =============================================================================
# 12. -v's line, for names that are not plain
# =============================================================================
# The line is built in a stack buffer inside the handler; a name that does not
# fit is written a piece at a time, and must read the same.

odd=$work/$(printf 'sl\377ep')
ln -s "$(command -v sleep)" "$odd"
run_case -v 0.3 "$odd" 10
deep=$work
for i in 1 2 3; do
  deep=$deep/$(printf '%0200d' "$i")
  mkdir -p "$deep"
done
ln -s "$(command -v sleep)" "$deep/sleep"
run_case -v 0.3 "$deep/sleep" 10
run_case -v -s RTMAX-1 0.3 "$deep/sleep" 10

# =============================================================================
# Summary
# =============================================================================
total=$((pass+fail+xfail+xpass))
printf 'timeout      %d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
  "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
